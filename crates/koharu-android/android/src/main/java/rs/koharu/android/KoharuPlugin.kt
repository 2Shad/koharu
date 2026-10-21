package rs.koharu.android

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.webkit.MimeTypeMap
import androidx.activity.result.ActivityResult
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.util.UUID
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class PickImportArgs {
  lateinit var extensions: Array<String>
}

@InvokeArg
class PickExportArgs {
  lateinit var suggestedName: String
}

@InvokeArg
class PickExportFileArgs {
  lateinit var suggestedName: String
  lateinit var mime: String
}

@InvokeArg
class ExportTarget {
  lateinit var kind: String
  lateinit var uri: String
}

@InvokeArg
class PublishExportArgs {
  lateinit var target: ExportTarget
  lateinit var paths: Array<String>
}

@InvokeArg
class ShareArgs {
  lateinit var paths: Array<String>
  lateinit var mime: String
}

@InvokeArg
class SecretKeyArgs {
  lateinit var key: String
}

@InvokeArg
class SecretSetArgs {
  lateinit var key: String
  lateinit var value: String
}

@InvokeArg
class ForegroundArgs {
  lateinit var title: String
  lateinit var text: String
}

@InvokeArg
class ForegroundProgressArgs {
  lateinit var title: String
  lateinit var text: String
  var progress: LongArray? = null
}

/**
 * Kotlin half of the `koharu-android` plugin. Every command is called from Rust, never from the
 * webview; the Rust side owns cancellation mapping and error propagation.
 */
@TauriPlugin
class KoharuPlugin(private val activity: Activity) : Plugin(activity) {
  companion object {
    private const val CANCELLED = "cancelled"
    private const val SECRET_PREFS = "koharu-secrets"
    private const val EXPORT_PREFS = "koharu-export"
    private const val KEY_ALIAS = "koharu-secrets"
    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    private const val CIPHER = "AES/GCM/NoPadding"
    private const val IV_SIZE = 12
  }

  /** Suggested folder name for the tree picked by `pickExportDirectory`. */
  private var pendingDirectoryName: String = ""

  @Command
  fun pickImport(invoke: Invoke) {
    try {
      // `extensions` is part of the Rust contract but is not used to filter the picker: providers
      // report .cbz/.rar as application/octet-stream or application/x-rar-compressed, so any MIME
      // filter hides supported files. The Rust importer rejects unsupported inputs by extension
      // and content.
      invoke.parseArgs(PickImportArgs::class.java)
      val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true)
        type = "*/*"
      }
      startActivityForResult(invoke, intent, "pickImportResult")
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to open the import picker", error)
    }
  }

  @ActivityCallback
  fun pickImportResult(invoke: Invoke, result: ActivityResult) {
    when (result.resultCode) {
      Activity.RESULT_OK -> {
        val uris = pickedUris(result.data)
        if (uris.isEmpty()) {
          cancel(invoke)
          return
        }
        runOnWorker(invoke) {
          val response = JSObject()
          response.put("paths", JSArray.from(copyIntoCache(uris).toTypedArray()))
          response
        }
      }
      Activity.RESULT_CANCELED -> cancel(invoke)
      else -> invoke.reject("Failed to pick import files")
    }
  }

  @Command
  fun pickExportDirectory(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(PickExportArgs::class.java)
      pendingDirectoryName = sanitize(args.suggestedName, "")
      val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
        addFlags(
          Intent.FLAG_GRANT_READ_URI_PERMISSION or
            Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
            Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
        )
      }
      startActivityForResult(invoke, intent, "pickExportDirectoryResult")
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to open the export directory picker", error)
    }
  }

  @ActivityCallback
  fun pickExportDirectoryResult(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode == Activity.RESULT_CANCELED || uri == null) {
      cancel(invoke)
      return
    }
    if (result.resultCode != Activity.RESULT_OK) {
      invoke.reject("Failed to pick an export directory")
      return
    }
    try {
      try {
        activity.contentResolver.takePersistableUriPermission(
          uri,
          Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
        )
      } catch (_: SecurityException) {
        // Providers that only grant a temporary tree permission still work for this session.
      }
      if (pendingDirectoryName.isNotEmpty()) {
        exportPrefs().edit().putString(treeKey(uri), pendingDirectoryName).apply()
      }
      resolveUri(invoke, uri)
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to keep the export directory", error)
    }
  }

  @Command
  fun pickExportFile(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(PickExportFileArgs::class.java)
      val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
        addCategory(Intent.CATEGORY_OPENABLE)
        type = args.mime.ifEmpty { "*/*" }
        putExtra(Intent.EXTRA_TITLE, sanitize(args.suggestedName))
      }
      startActivityForResult(invoke, intent, "pickExportFileResult")
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to open the export file picker", error)
    }
  }

  @ActivityCallback
  fun pickExportFileResult(invoke: Invoke, result: ActivityResult) {
    val uri = result.data?.data
    if (result.resultCode == Activity.RESULT_CANCELED || uri == null) {
      cancel(invoke)
      return
    }
    if (result.resultCode != Activity.RESULT_OK) {
      invoke.reject("Failed to pick an export file")
      return
    }
    resolveUri(invoke, uri)
  }

  @Command
  fun publishExport(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(PublishExportArgs::class.java)
      val files = args.paths.map { File(it) }
      runOnWorker(invoke) {
        when (args.target.kind) {
          "directory" -> publishIntoTree(Uri.parse(args.target.uri), files)
          "file" -> {
            if (files.size != 1) {
              throw IOException("A single-file export target received ${files.size} files")
            }
            writeIntoDocument(Uri.parse(args.target.uri), files.first())
          }
          else -> throw IOException("Unknown export target: ${args.target.kind}")
        }
        null
      }
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to publish the export", error)
    }
  }

  @Command
  fun share(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(ShareArgs::class.java)
      if (args.paths.isEmpty()) throw IOException("No files to share")
      val authority = "${activity.packageName}.fileprovider"
      val uris = args.paths.map {
        FileProvider.getUriForFile(activity, authority, File(it))
      }
      val send = if (uris.size == 1) {
        Intent(Intent.ACTION_SEND).apply {
          type = args.mime.ifEmpty { "*/*" }
          putExtra(Intent.EXTRA_STREAM, uris.first())
        }
      } else {
        Intent(Intent.ACTION_SEND_MULTIPLE).apply {
          type = args.mime.ifEmpty { "*/*" }
          putParcelableArrayListExtra(Intent.EXTRA_STREAM, ArrayList(uris))
        }
      }
      send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      val chooser = Intent.createChooser(send, null)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION)
      activity.applicationContext.startActivity(chooser)
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to open the share sheet", error)
    }
  }

  @Command
  fun secretGet(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(SecretKeyArgs::class.java)
      val response = JSObject()
      response.put("value", secretPrefs().getString(args.key, null)?.let { readSecret(args.key, it) })
      invoke.resolve(response)
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to read the stored secret", error)
    }
  }

  /**
   * Reads a stored ciphertext. A value that cannot be decrypted (Keystore key lost, entry
   * restored from Android Auto Backup) is dropped so a stale entry cannot brick startup.
   */
  private fun readSecret(key: String, stored: String): String? = try {
    decrypt(stored)
  } catch (_: GeneralSecurityException) {
    secretPrefs().edit().remove(key).apply()
    null
  } catch (_: IOException) {
    secretPrefs().edit().remove(key).apply()
    null
  }

  @Command
  fun secretSet(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(SecretSetArgs::class.java)
      secretPrefs().edit().putString(args.key, encrypt(args.value)).apply()
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to store the secret", error)
    }
  }

  @Command
  fun secretDelete(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(SecretKeyArgs::class.java)
      secretPrefs().edit().remove(args.key).apply()
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to delete the secret", error)
    }
  }

  @Command
  fun background(invoke: Invoke) {
    activity.runOnUiThread { activity.moveTaskToBack(true) }
    invoke.resolve()
  }

  @Command
  fun foregroundStart(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(ForegroundArgs::class.java)
      // The foreground-service notification is optional: request the permission on Android 13+
      // and start regardless of the result.
      if (
        Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
        activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
          PackageManager.PERMISSION_GRANTED
      ) {
        activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
      }
      ForegroundService.start(activity, args.title, args.text)
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to start background work", error)
    }
  }

  @Command
  fun foregroundUpdate(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(ForegroundProgressArgs::class.java)
      val progress = args.progress
      val done = progress?.getOrNull(0) ?: -1
      val total = progress?.getOrNull(1) ?: -1
      if (!ForegroundService.update(args.title, args.text, done, total)) {
        ForegroundService.start(activity, args.title, args.text, done, total)
      }
      invoke.resolve()
    } catch (error: Exception) {
      invoke.reject(error.message ?: "Failed to update background work", error)
    }
  }

  @Command
  fun foregroundStop(invoke: Invoke) {
    ForegroundService.stop(activity)
    invoke.resolve()
  }

  private fun cancel(invoke: Invoke) {
    invoke.reject("cancelled", CANCELLED, null, null)
  }

  private fun resolveUri(invoke: Invoke, uri: Uri) {
    val response = JSObject()
    response.put("uri", uri.toString())
    invoke.resolve(response)
  }

  private fun runOnWorker(invoke: Invoke, block: () -> JSObject?) {
    Thread {
      try {
        val result = block()
        if (result == null) invoke.resolve() else invoke.resolve(result)
      } catch (error: Exception) {
        invoke.reject(error.message ?: "Koharu platform command failed", error)
      }
    }.start()
  }

  private fun pickedUris(intent: Intent?): List<Uri> {
    if (intent == null) return emptyList()
    val clip = intent.clipData
    if (clip != null) return (0 until clip.itemCount).map { clip.getItemAt(it).uri }
    return listOfNotNull(intent.data)
  }

  private fun copyIntoCache(uris: List<Uri>): List<String> {
    val directory = File(File(activity.cacheDir, "import"), UUID.randomUUID().toString())
    if (!directory.mkdirs()) throw IOException("Cannot create ${directory.absolutePath}")
    try {
      val used = mutableSetOf<String>()
      return uris.map { uri ->
        val name = uniqueName(sanitize(displayName(uri) ?: "import"), used)
        val destination = File(directory, name)
        val input = activity.contentResolver.openInputStream(uri)
          ?: throw IOException("Cannot read $uri")
        input.use { source -> destination.outputStream().use { source.copyTo(it) } }
        destination.absolutePath
      }
    } catch (error: Exception) {
      directory.deleteRecursively()
      throw error
    }
  }

  private fun displayName(uri: Uri): String? {
    activity.contentResolver.query(
      uri,
      arrayOf(OpenableColumns.DISPLAY_NAME),
      null,
      null,
      null,
    )?.use { cursor ->
      if (cursor.moveToFirst()) return cursor.getString(0)
    }
    return uri.lastPathSegment
  }

  private fun publishIntoTree(tree: Uri, files: List<File>) {
    val resolver = activity.contentResolver
    var parent = DocumentsContract.buildDocumentUriUsingTree(
      tree,
      DocumentsContract.getTreeDocumentId(tree),
    )
    val directoryName = exportPrefs().getString(treeKey(tree), null).orEmpty()
    if (directoryName.isNotEmpty()) {
      parent = findChild(tree, parent, directoryName, true)
        ?: DocumentsContract.createDocument(
          resolver,
          parent,
          DocumentsContract.Document.MIME_TYPE_DIR,
          directoryName,
        )
        ?: throw IOException("Cannot create the export folder \"$directoryName\"")
    }
    for (file in files) {
      if (!file.isFile) throw IOException("Missing export file ${file.absolutePath}")
      val name = sanitize(file.name)
      val document = findChild(tree, parent, name, false)
        ?: DocumentsContract.createDocument(resolver, parent, mimeForName(name), name)
        ?: throw IOException("Cannot create \"$name\" in the export folder")
      resolver.openOutputStream(document, "wt")?.use { output ->
        file.inputStream().use { it.copyTo(output) }
      } ?: throw IOException("Cannot write \"$name\"")
    }
  }

  private fun writeIntoDocument(document: Uri, file: File) {
    activity.contentResolver.openOutputStream(document, "wt")?.use { output ->
      file.inputStream().use { it.copyTo(output) }
    } ?: throw IOException("Cannot write ${file.name}")
  }

  private fun findChild(tree: Uri, parent: Uri, name: String, directory: Boolean): Uri? {
    val children = DocumentsContract.buildChildDocumentsUriUsingTree(
      tree,
      DocumentsContract.getDocumentId(parent),
    )
    val projection = arrayOf(
      DocumentsContract.Document.COLUMN_DISPLAY_NAME,
      DocumentsContract.Document.COLUMN_MIME_TYPE,
      DocumentsContract.Document.COLUMN_DOCUMENT_ID,
    )
    activity.contentResolver.query(children, projection, null, null, null)?.use { cursor ->
      while (cursor.moveToNext()) {
        if (cursor.getString(0) != name) continue
        val isDirectory = cursor.getString(1) == DocumentsContract.Document.MIME_TYPE_DIR
        if (isDirectory != directory) continue
        return DocumentsContract.buildDocumentUriUsingTree(tree, cursor.getString(2))
      }
    }
    return null
  }

  private fun exportPrefs() = activity.getSharedPreferences(EXPORT_PREFS, Activity.MODE_PRIVATE)

  private fun treeKey(uri: Uri) = "tree:$uri"

  private fun secretPrefs() = activity.getSharedPreferences(SECRET_PREFS, Activity.MODE_PRIVATE)

  private fun encrypt(value: String): String {
    val cipher = Cipher.getInstance(CIPHER)
    cipher.init(Cipher.ENCRYPT_MODE, secretKey())
    val encrypted = cipher.doFinal(value.toByteArray(Charsets.UTF_8))
    return Base64.encodeToString(cipher.iv + encrypted, Base64.NO_WRAP)
  }

  private fun decrypt(encoded: String): String {
    val bytes = Base64.decode(encoded, Base64.NO_WRAP)
    if (bytes.size <= IV_SIZE) throw IOException("The stored secret is corrupted")
    val cipher = Cipher.getInstance(CIPHER)
    cipher.init(Cipher.DECRYPT_MODE, secretKey(), GCMParameterSpec(128, bytes, 0, IV_SIZE))
    return String(cipher.doFinal(bytes, IV_SIZE, bytes.size - IV_SIZE), Charsets.UTF_8)
  }

  private fun secretKey(): SecretKey {
    val store = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
    (store.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let { return it.secretKey }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
    generator.init(
      KeyGenParameterSpec.Builder(
        KEY_ALIAS,
        KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
      )
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build(),
    )
    return generator.generateKey()
  }

  private fun sanitize(name: String, fallback: String = "file"): String {
    val trimmed = name
      .substringAfterLast('/')
      .substringAfterLast('\\')
      .replace("\u0000", "")
      .trim()
    return trimmed.ifEmpty { fallback }
  }

  private fun uniqueName(name: String, used: MutableSet<String>): String {
    if (used.add(name)) return name
    val base = name.substringBeforeLast('.', name)
    val extension = name.substringAfterLast('.', "")
    var index = 1
    while (true) {
      val candidate = if (extension.isEmpty()) "$base ($index)" else "$base ($index).$extension"
      if (used.add(candidate)) return candidate
      index++
    }
  }

  private fun mimeForName(name: String): String {
    val extension = name.substringAfterLast('.', "")
    if (extension.isEmpty()) return "application/octet-stream"
    return MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension.lowercase())
      ?: "application/octet-stream"
  }
}

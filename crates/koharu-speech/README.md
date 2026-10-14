# Koharu Speech

Voices translated manga dialogue with Fish Audio
[Drama 3](https://huggingface.co/fishaudio/drama-3).

- **Direction.** `direction` builds the post-translation LLM prompt that inserts
  cues such as `[sighs]` and names speakers. `direction::parse` keeps a line's
  translation when the model changes its words; `script::sanitize` strips
  Drama 3 control tokens (`<|speaker:0|>`). The caller owns the LLM transport
  (`koharu-pipeline` uses `Translator::complete`).
- **Voicing.** `Speech::synthesize` returns WAV bytes from a private
  `audiocpp_server` on loopback. The server restarts when its settings change
  and dies with Koharu (Windows job object, Linux `PR_SET_PDEATHSIG`).

## Runtime

- Server: `koharu-runtime` downloads `audiocpp_server` from the
  [karrot0/audio.cpp](https://github.com/karrot0/audio.cpp) release pinned in
  `audiocpp.rs`, plus its CUDA 13 libraries from NVIDIA wheels. Requires an
  NVIDIA GPU.
- Model: `Jmica/Drama-3-GGUF` Q2_K (17 GB), pinned. Downloaded only by
  Settings → Speech → Download (`Speech::install`).
- `cpu_expert_layers` defaults from VRAM: 0 at 24 GB, 8 at 16 GB, 16 at 12 GB.

Local copies override either download:

```toml
[speech]
server = 'C:\path\to\audiocpp_server.exe'
model = 'C:\path\to\drama-3-backbone-8bit-experts-Q2_K.gguf'
cpu_expert_layers = 16
```

Voice one line:

```powershell
cargo run -p koharu-speech --bin speak -- "[sighs] Fine. If we miss it, it's on you. [laughs] Well, maybe that's fair." --out line.wav
```

`voices` entries clone a character from a reference WAV and its exact
transcript; other characters get a stable per-name seed.

Model and derivatives: Fish Audio Community License.

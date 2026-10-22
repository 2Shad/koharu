import type { Metadata, Viewport } from 'next'
import localFont from 'next/font/local'
import { ThemeProvider } from 'next-themes'

import './globals.css'
import Providers from '@/app/providers'

const notoSansCJK = localFont({
  src: './fonts/NotoSansCJK-VF.otf',
  weight: '100 900',
  variable: '--font-noto-sans-cjk',
})

export const metadata: Metadata = {
  title: 'Koharu',
}

// Touch layout: full-bleed past the notch and resize the layout viewport when the soft
// keyboard opens so text fields inside the mobile sheets stay visible.
export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
  viewportFit: 'cover',
  interactiveWidget: 'resizes-content',
}

function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang='en-US' suppressHydrationWarning style={{ backgroundColor: 'transparent' }}>
      <body
        className={`${notoSansCJK.variable} antialiased`}
        style={{ backgroundColor: 'transparent' }}
      >
        <ThemeProvider attribute='class' defaultTheme='system' enableSystem>
          <Providers>{children}</Providers>
        </ThemeProvider>
      </body>
    </html>
  )
}

export default RootLayout

import type { Metadata } from "next";
import "./globals.css";
import { ScanProvider } from "@/lib/scan-store";

export const metadata: Metadata = {
  title: "Alpha Radar",
  description: "Swing Entry Confluence Scanner",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  // UI strings are Japanese per the project language policy. ScanProvider lives
  // in the layout so scan results survive list ↔ chart navigation.
  return (
    <html lang="ja">
      <body>
        <ScanProvider>{children}</ScanProvider>
      </body>
    </html>
  );
}

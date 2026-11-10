// A WebKit reference frame: loads a page in an offscreen WKWebView at a device pixel ratio of one and writes what
// it draws as an sRGB PNG. This is the system WebKit - Safari's engine, rasterizing with Core Graphics.
//
//   swiftc -O wk_shot.swift -o wk_shot   (refs.py webkit does this into OUT/bin)
//   wk_shot page.html out.png 460 260
import AppKit
import WebKit

final class Shot: NSObject, WKNavigationDelegate {
    let web: WKWebView
    let out: URL
    let size: CGSize

    init(page: URL, out: URL, size: CGSize) {
        web = WKWebView(frame: CGRect(origin: .zero, size: size), configuration: WKWebViewConfiguration())
        self.out = out
        self.size = size
        super.init()
        // One device pixel per CSS pixel, whatever the display (the setter is _setOverrideDeviceScaleFactor:).
        web.setValue(1.0, forKey: "overrideDeviceScaleFactor")
        web.navigationDelegate = self
        web.loadFileURL(page, allowingReadAccessTo: URL(fileURLWithPath: "/"))
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        // Two animation frames, so that what is snapshotted has been painted.
        webView.evaluateJavaScript("new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(devicePixelRatio))))") { _, _ in
            self.snapshot()
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.0) { self.snapshot() }
    }

    var taken = false
    func snapshot() {
        if taken { return }
        taken = true
        let config = WKSnapshotConfiguration()
        config.rect = CGRect(origin: .zero, size: size)
        config.snapshotWidth = NSNumber(value: Double(size.width))
        web.takeSnapshot(with: config) { image, error in
            guard let image, let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
                FileHandle.standardError.write("snapshot failed: \(String(describing: error))\n".data(using: .utf8)!)
                exit(2)
            }
            let rep = NSBitmapImageRep(cgImage: cg)
            let srgb = rep.converting(to: .sRGB, renderingIntent: .absoluteColorimetric) ?? rep
            guard let png = srgb.representation(using: .png, properties: [:]) else { exit(3) }
            try! png.write(to: self.out)
            print("\(cg.width)x\(cg.height) \(cg.colorSpace?.name as String? ?? "?")")
            exit(0)
        }
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { exit(4) }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { exit(4) }
}

let args = CommandLine.arguments
guard args.count == 5, let w = Double(args[3]), let h = Double(args[4]) else {
    print("usage: wk_shot page.html out.png width height")
    exit(1)
}
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let shot = Shot(page: URL(fileURLWithPath: args[1]), out: URL(fileURLWithPath: args[2]), size: CGSize(width: w, height: h))
DispatchQueue.main.asyncAfter(deadline: .now() + 30) { exit(5) }
app.run()

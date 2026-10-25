// Run a JS file inside a WKWebView at a given size and print its result.
//   swift webkit-eval.swift <url> <width> <height> <script.js>
// The script's last expression is returned (wrap in an async IIFE and return a promise if needed).
import Cocoa
import WebKit

let args = CommandLine.arguments
let url = args[1], width = Double(args[2])!, height = Double(args[3])!
let script = try! String(contentsOfFile: args[4], encoding: .utf8)

let app = NSApplication.shared
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: width, height: height))
let win = NSWindow(contentRect: web.frame, styleMask: [.titled], backing: .buffered, defer: false)
win.contentView = web
// On screen, not just created: a window WebKit thinks is hidden pauses requestAnimationFrame.
win.orderFront(nil)

// Never hang a CI job: give up after two minutes, and on any failed page load.
DispatchQueue.main.asyncAfter(deadline: .now() + 120) {
    print("timeout: no result in 120 s"); fflush(stdout)
    exit(1)
}

class Delegate: NSObject, WKNavigationDelegate {
    func webView(_ w: WKWebView, didFail n: WKNavigation!, withError error: Error) {
        print("load failed: \(error)"); fflush(stdout)
        exit(1)
    }
    func webView(_ w: WKWebView, didFailProvisionalNavigation n: WKNavigation!, withError error: Error) {
        print("load failed: \(error)"); fflush(stdout)
        exit(1)
    }
    func webView(_ w: WKWebView, didFinish n: WKNavigation!) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
            w.callAsyncJavaScript(script, arguments: [:], in: nil, in: .page) { result in
                switch result {
                case .success(let value): print(value); fflush(stdout)
                case .failure(let error): print("JS error: \(error)"); fflush(stdout); exit(1)
                }
                exit(0)
            }
        }
    }
}
let delegate = Delegate()
web.navigationDelegate = delegate
web.load(URLRequest(url: URL(string: url)!))
app.run()

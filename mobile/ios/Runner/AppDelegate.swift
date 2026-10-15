import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)

    // Keeps the Tailscale state directory out of iCloud and device backups.
    // The key can be reset by file operations, so Dart calls this on every launch.
    let channel = FlutterMethodChannel(
      name: "jp.infodesk.mobile/backup",
      binaryMessenger: engineBridge.applicationRegistrar.messenger())
    channel.setMethodCallHandler { call, result in
      guard call.method == "exclude", let path = call.arguments as? String else {
        result(FlutterMethodNotImplemented)
        return
      }
      var url = URL(fileURLWithPath: path, isDirectory: true)
      do {
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try url.setResourceValues(values)
        let back = try url.resourceValues(forKeys: [.isExcludedFromBackupKey])
        result(back.isExcludedFromBackup == true)
      } catch {
        result(false)
      }
    }
  }
}

// Placeholder that references one symbol from each package the app will use,
// so the scaffold compiles and links all of them (including tailscale's Go
// build hook). Replaced by the real code from Task 1 on.
import 'package:archive/archive.dart';
import 'package:extension_google_sign_in_as_googleapis_auth/extension_google_sign_in_as_googleapis_auth.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:google_sign_in/google_sign_in.dart';
import 'package:googleapis/drive/v3.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:sqflite/sqflite.dart';
import 'package:tailscale/tailscale.dart';
import 'package:url_launcher/url_launcher.dart';

/// Symbols from the packages in the plan's tech stack.
List<Object> linkedPackages() => <Object>[
  GoogleSignIn,
  DriveApi,
  Tailscale,
  Database,
  http.Client,
  launchUrl,
  FlutterSecureStorage,
  getApplicationSupportDirectory,
  GZipDecoder,
  driveClient,
];

/// An authenticated client for the Drive app data folder.
http.Client driveClient(GoogleSignInClientAuthorization authorization) =>
    authorization.authClient(scopes: <String>[DriveApi.driveAppdataScope]);

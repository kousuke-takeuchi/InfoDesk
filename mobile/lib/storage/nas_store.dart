import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:tailscale/tailscale.dart';
import 'package:xml/xml.dart';

import 'drive_store.dart';
import 'tailnet_port.dart';
import 'timeouts.dart';

const nasUrlMessage =
    'NAS の URL を確かめてください (例: http://takeuchi-nas:5005/infodesk/)';
const nasHttpsMessage = 'http:// を使ってください (Tailscale の中なので暗号化されています)';
const nasNotInTailnetMessage = 'NAS が Tailscale の中に見つかりません。NAS の名前を確かめてください';
const nasBadCredentialsMessage = 'NAS のユーザー名かパスワードが違います';
const nasForbiddenMessage = 'NAS のフォルダに書き込む権限がありません';
const nasStorageFullMessage = 'NAS の空き容量が足りません';
const nasTailscaleLoginMessage = 'Tailscale にログインしてください';
const nasRedirectMessage = 'NAS の URL を確かめてください (転送されました)';

const _propfindBody =
    '<?xml version="1.0" encoding="utf-8"?><D:propfind xmlns:D="DAV:"><D:prop>'
    '<D:getlastmodified/><D:getcontentlength/><D:resourcetype/></D:prop></D:propfind>';

/// WebDAV folder on the NAS, reached only through the tailnet. A file's name is
/// its id. [list] returns plain files directly inside the folder; filtering by
/// `parseFileName` is left to the sync pass.
class NasStore implements DriveStore {
  NasStore({
    required this.tailnet,
    required this.url,
    required this.user,
    required this.password,
    this.connectTimeout = defaultConnectTimeout,
    this.requestTimeout = defaultRequestTimeout,
  });

  final TailnetPort tailnet;
  final String url;
  final String user;
  final String password;

  /// Limit for response headers, and for the whole request with its body.
  final Duration connectTimeout;
  final Duration requestTimeout;

  /// The folder URL with its host replaced by the peer's 100.x address.
  /// Throws before any request when the URL is not usable.
  Future<Uri> _folder() async {
    final Uri u;
    try {
      u = Uri.parse(url.trim());
    } on FormatException {
      throw const StorageError(StorageErrorKind.other, nasUrlMessage);
    }
    if (u.scheme == 'https') {
      throw const StorageError(StorageErrorKind.other, nasHttpsMessage);
    }
    if (u.scheme != 'http' || u.host.isEmpty) {
      throw const StorageError(StorageErrorKind.other, nasUrlMessage);
    }
    InternetAddress? addr;
    try {
      addr = await tailnet.peerAddr(u.host);
    } on TailscaleException catch (e) {
      throw _tailscaleError(e);
    } on Exception {
      addr = null;
    }
    if (addr == null) {
      throw const StorageError(StorageErrorKind.other, nasNotInTailnetMessage);
    }
    final segs = u.pathSegments.where((s) => s.isNotEmpty).toList();
    return Uri(
      scheme: 'http',
      host: addr.address,
      port: u.hasPort ? u.port : null,
      pathSegments: [...segs, ''],
      query: u.hasQuery ? u.query : null,
    );
  }

  Uri _fileUri(Uri folder, String name) {
    if (!_safeName(name)) {
      throw StorageError(StorageErrorKind.other, '保存先のエラー: 使えないファイル名です');
    }
    final segs = folder.pathSegments.where((s) => s.isNotEmpty).toList();
    return folder.replace(pathSegments: [...segs, name]);
  }

  Future<http.Response> _send(
    String method,
    Uri uri, {
    String? depth,
    List<int>? body,
    String? contentType,
  }) async {
    final req = http.Request(method, uri)
      ..followRedirects = false
      ..headers['Authorization'] =
          'Basic ${base64Encode(utf8.encode('$user:$password'))}';
    if (depth != null) req.headers['Depth'] = depth;
    if (contentType != null) req.headers['Content-Type'] = contentType;
    if (body != null) req.bodyBytes = body;
    final http.Response resp;
    try {
      resp = await (() async {
        final client = await tailnet.client();
        final streamed = await client.send(req).timeout(connectTimeout);
        return http.Response.fromStream(streamed);
      })().timeout(requestTimeout);
    } on TailscaleException catch (e) {
      throw _tailscaleError(e);
    } on http.ClientException {
      throw const StorageError(StorageErrorKind.retryable, networkMessage);
    } on SocketException {
      throw const StorageError(StorageErrorKind.retryable, networkMessage);
    } on TimeoutException {
      throw const StorageError(StorageErrorKind.retryable, networkMessage);
    }
    final code = resp.statusCode;
    if (code >= 200 && code < 300) return resp;
    if (code == 401) {
      throw const StorageError(
        StorageErrorKind.badCredentials,
        nasBadCredentialsMessage,
      );
    }
    if (code >= 300 && code < 400) {
      throw const StorageError(StorageErrorKind.other, nasRedirectMessage);
    }
    if (code == 507 || code == 413) {
      throw const StorageError(StorageErrorKind.other, nasStorageFullMessage);
    }
    if (code == 403) {
      throw const StorageError(StorageErrorKind.other, nasForbiddenMessage);
    }
    if (code == 408 || code == 429 || code >= 500) {
      throw const StorageError(StorageErrorKind.retryable, networkMessage);
    }
    throw StorageError(StorageErrorKind.other, '保存先のエラー: HTTP $code');
  }

  Future<List<_Entry>> _propfind(Uri uri, String depth) async {
    final resp = await _send(
      'PROPFIND',
      uri,
      depth: depth,
      body: utf8.encode(_propfindBody),
      contentType: 'application/xml',
    );
    if (resp.statusCode != 207) throw _unexpectedPropfind();
    return _parseMultistatus(utf8.decode(resp.bodyBytes, allowMalformed: true));
  }

  @override
  Future<List<RemoteFile>> list() async {
    final folder = await _folder();
    final entries = await _propfind(folder, '1');
    // pathSegments are already decoded; decoding again would corrupt a
    // literal `%` in the folder name.
    final base = '/${folder.pathSegments.where((s) => s.isNotEmpty).join('/')}';
    final decodedBase = base.replaceAll(RegExp(r'/+$'), '');
    final files = <RemoteFile>[];
    for (final e in entries) {
      if (e.isCollection) continue;
      final path = _percentDecode(_hrefPath(e.href));
      final trimmed = path.replaceAll(RegExp(r'/+$'), '');
      if (trimmed.length < decodedBase.length ||
          trimmed.substring(0, decodedBase.length).toLowerCase() !=
              decodedBase.toLowerCase()) {
        continue;
      }
      final rest = trimmed.substring(decodedBase.length);
      if (!rest.startsWith('/')) continue; // the folder itself, or a sibling
      final name = rest.substring(1);
      if (!_safeName(name)) continue;
      final f = _toRemote(name, e);
      if (f != null) files.add(f);
    }
    return files;
  }

  @override
  Future<List<int>> download(RemoteFile f) async {
    final folder = await _folder();
    final resp = await _send('GET', _fileUri(folder, f.id));
    return resp.bodyBytes;
  }

  @override
  Future<RemoteFile> create(String name, List<int> bytes, String mime) async {
    final folder = await _folder();
    final uri = _fileUri(folder, name);
    await _send('PUT', uri, body: bytes, contentType: mime);
    final entries = await _propfind(uri, '0');
    final f = entries.isEmpty ? null : _toRemote(name, entries.first);
    if (f == null) {
      throw StorageError(StorageErrorKind.other, '保存先のエラー: $name の情報を読めません');
    }
    return f;
  }
}

/// Never surfaces the exception text: it is English and internal.
StorageError _tailscaleError(TailscaleException e) =>
    e is TailscaleUsageException
    ? const StorageError(StorageErrorKind.other, nasTailscaleLoginMessage)
    : const StorageError(StorageErrorKind.retryable, networkMessage);

StorageError _unexpectedPropfind() => const StorageError(
  StorageErrorKind.other,
  '保存先のエラー: unexpected PROPFIND response',
);

/// A name is one plain path segment: no separators, no `.` or `..`.
bool _safeName(String name) =>
    !(name.isEmpty ||
        name == '.' ||
        name == '..' ||
        name.contains('/') ||
        name.contains(r'\'));

class _Entry {
  String href = '';
  String? lastModified;
  int? length;
  bool isCollection = false;
}

RemoteFile? _toRemote(String name, _Entry e) {
  final raw = e.lastModified;
  if (raw == null) return null;
  final parsed = _parseHttpDate(raw.trim());
  if (parsed == null) return null;
  final t = parsed;
  return RemoteFile(
    id: name,
    name: name,
    modifiedTime: rfc3339Seconds(t),
    size: e.length,
  );
}

String _percentDecode(String s) {
  final b = utf8.encode(s);
  final out = <int>[];
  int? hex(int c) {
    if (c >= 0x30 && c <= 0x39) return c - 0x30;
    if (c >= 0x41 && c <= 0x46) return c - 0x41 + 10;
    if (c >= 0x61 && c <= 0x66) return c - 0x61 + 10;
    return null;
  }

  var i = 0;
  while (i < b.length) {
    if (b[i] == 0x25 && i + 2 < b.length) {
      final h = hex(b[i + 1]);
      final l = hex(b[i + 2]);
      if (h != null && l != null) {
        out.add(h * 16 + l);
        i += 3;
        continue;
      }
    }
    out.add(b[i]);
    i++;
  }
  return utf8.decode(out, allowMalformed: true);
}

/// The path part of an `href`, which may be absolute (`http://host:port/path`).
String _hrefPath(String href) {
  final h = href.trim();
  final i = h.indexOf('://');
  if (i < 0) return h;
  final j = h.indexOf('/', i + 3);
  return j < 0 ? '/' : h.substring(j);
}

/// Matches elements by local name, so any prefix for the `DAV:` namespace
/// (`D:`, `d:`, none) works. Only a `propstat` with status 200 (or none) counts.
List<_Entry> _parseMultistatus(String text) {
  final XmlDocument doc;
  try {
    doc = XmlDocument.parse(text);
  } on XmlException {
    throw _unexpectedPropfind();
  }
  if (doc.rootElement.name.local != 'multistatus') throw _unexpectedPropfind();

  Iterable<XmlElement> kids(XmlElement e, String n) =>
      e.childElements.where((c) => c.name.local == n);
  String? firstText(XmlElement e, String n) {
    final k = kids(e, n);
    return k.isEmpty ? null : k.first.innerText.trim();
  }

  final out = <_Entry>[];
  for (final r in kids(doc.rootElement, 'response')) {
    final e = _Entry()..href = firstText(r, 'href') ?? '';
    for (final ps in kids(r, 'propstat')) {
      final status = firstText(ps, 'status');
      final ok =
          status == null ||
          status.split(RegExp(r'\s+')).elementAtOrNull(1) == '200';
      if (!ok) continue;
      for (final prop in kids(ps, 'prop')) {
        if (prop.descendantElements.any((d) => d.name.local == 'collection')) {
          e.isCollection = true;
        }
        e.lastModified ??= firstText(prop, 'getlastmodified');
        e.length ??= int.tryParse(firstText(prop, 'getcontentlength') ?? '');
      }
    }
    out.add(e);
  }
  return out;
}

const _months = [
  'jan', 'feb', 'mar', 'apr', 'may', 'jun',
  'jul', 'aug', 'sep', 'oct', 'nov', 'dec', //
];

final _rfc2822 = RegExp(
  r'^(?:[A-Za-z]{3,9},\s*)?(\d{1,2})\s+([A-Za-z]{3})\s+(\d{2,4})\s+'
  r'(\d{1,2}):(\d{2})(?::(\d{2}))?\s*([+-]\d{4}|GMT|UT|Z)?$',
  caseSensitive: false,
);

/// The `dart:io` formats (RFC 1123, RFC 850, asctime), then RFC 2822 with a
/// numeric offset and an optional weekday, as the Mac's `parse_from_rfc2822`
/// accepts. Null when neither fits.
DateTime? _parseHttpDate(String s) {
  try {
    return HttpDate.parse(s);
  } on Exception {
    // fall through
  }
  final m = _rfc2822.firstMatch(s);
  if (m == null) return null;
  final month = _months.indexOf(m[2]!.toLowerCase()) + 1;
  if (month == 0) return null;
  var year = int.parse(m[3]!);
  if (m[3]!.length == 2) year += year < 50 ? 2000 : 1900;
  final zone = m[7];
  var offsetMin = 0;
  if (zone != null && (zone.startsWith('+') || zone.startsWith('-'))) {
    final sign = zone.startsWith('-') ? -1 : 1;
    offsetMin =
        sign *
        (int.parse(zone.substring(1, 3)) * 60 + int.parse(zone.substring(3)));
  }
  final day = int.parse(m[1]!);
  final h = int.parse(m[4]!);
  final mi = int.parse(m[5]!);
  final sec = int.parse(m[6] ?? '0');
  if (day < 1 || day > 31 || h > 23 || mi > 59 || sec > 60) return null;
  final t = DateTime.utc(year, month, day, h, mi, sec);
  // DateTime.utc rolls over invalid days (31 Feb); reject those.
  if (t.month != month) return null;
  return t.subtract(Duration(minutes: offsetMin));
}

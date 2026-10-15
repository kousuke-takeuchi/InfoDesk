import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:infodesk_mobile/storage/drive_store.dart';
import 'package:infodesk_mobile/storage/nas_store.dart';
import 'package:infodesk_mobile/storage/tailnet_port.dart';

class FakeTailnet implements TailnetPort {
  FakeTailnet(this.handler, {this.peers = const {'nas': '100.64.0.7'}});
  final Future<http.Response> Function(http.Request) handler;
  final Map<String, String> peers;
  final requests = <http.Request>[];
  int clientCalls = 0;

  @override
  Future<http.Client> client() async {
    clientCalls++;
    return MockClient((req) async {
      requests.add(req);
      return handler(req);
    });
  }

  @override
  Future<InternetAddress?> peerAddr(String host) async {
    final ip = peers[host];
    return ip == null ? null : InternetAddress(ip);
  }
}

const synology = '''<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
<D:response>
<D:href>/InfoDesk/</D:href>
<D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:00:00 GMT</D:getlastmodified><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
</D:response>
<D:response>
<D:href>/InfoDesk/manifest.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:00:01 GMT</D:getlastmodified><D:getcontentlength>42</D:getcontentlength><D:resourcetype/></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
</D:response>
<D:response>
<D:href>/InfoDesk/sub/</D:href>
<D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 03:00:02 GMT</D:getlastmodified><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
</D:response>
<D:response>
<D:href>/InfoDesk/day-2026-10-14.json.gz</D:href>
<D:propstat><D:prop><D:getlastmodified>Wed, 14 Oct 2026 04:30:00 GMT</D:getlastmodified><D:getcontentlength>7</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
</D:response>
</D:multistatus>''';

http.Response multistatus(String body, [int status = 207]) => http.Response(
  body,
  status,
  headers: {'content-type': 'application/xml; charset=utf-8'},
);

NasStore store(FakeTailnet t, {String url = 'http://nas:5005/InfoDesk/'}) =>
    NasStore(tailnet: t, url: url, user: 'kou', password: 'pw');

Matcher nasError(StorageErrorKind kind, [String? message]) =>
    isA<StorageError>()
        .having((e) => e.kind, 'kind', kind)
        .having((e) => e.message, 'message', message ?? isA<String>());

void main() {
  group('list', () {
    test('parses a Synology multistatus, skipping folders', () async {
      final t = FakeTailnet((_) async => multistatus(synology));
      final files = await store(t).list();
      expect(files.map((f) => f.name), [
        'manifest.json',
        'day-2026-10-14.json.gz',
      ]);
      expect(files[0].id, 'manifest.json');
      expect(files[0].size, 42);
      expect(files[0].modifiedTime, '2026-10-14T03:00:01Z');
      expect(files[1].modifiedTime, '2026-10-14T04:30:00Z');
      final r = t.requests.single;
      expect(r.method, 'PROPFIND');
      expect(r.headers['Depth'], '1');
      expect(r.body, contains('getlastmodified'));
      expect(r.followRedirects, isFalse);
    });

    test('request goes to the 100.x address, keeping port and path', () async {
      final t = FakeTailnet((_) async => multistatus(synology));
      await store(t).list();
      final u = t.requests.single.url;
      expect(u.host, '100.64.0.7');
      expect(u.port, 5005);
      expect(u.path, '/InfoDesk/');
      expect(u.scheme, 'http');
    });

    test('a folder URL without a trailing slash resolves alike', () async {
      final t = FakeTailnet((_) async => multistatus(synology));
      final files = await store(t, url: 'http://nas:5005/InfoDesk').list();
      expect(files, hasLength(2));
      expect(t.requests.single.url.path, '/InfoDesk/');
    });

    test(
      'other prefixes, absolute href with a port, percent-decoding',
      () async {
        const body = '''<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
<d:response><d:href>http://100.64.0.7:5005/Info%44esk/</d:href>
<d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>http://100.64.0.7:5005/Info%44esk/ops-d%31-1.json</d:href>
<d:propstat><d:prop><d:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</d:getlastmodified><d:getcontentlength>2</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>''';
        final t = FakeTailnet((_) async => multistatus(body));
        final files = await store(t).list();
        expect(files.single.name, 'ops-d1-1.json');
      },
    );

    test('default namespace without a prefix works', () async {
      const body = '''<multistatus xmlns="DAV:">
<response><href>/InfoDesk/manifest.json</href>
<propstat><prop><getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</getlastmodified></prop><status>HTTP/1.1 200 OK</status></propstat></response>
</multistatus>''';
      final t = FakeTailnet((_) async => multistatus(body));
      expect((await store(t).list()).single.name, 'manifest.json');
    });

    test('the folder name matches case-insensitively', () async {
      const body = '''<D:multistatus xmlns:D="DAV:">
<D:response><D:href>/infodesk/manifest.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>''';
      final t = FakeTailnet((_) async => multistatus(body));
      expect(await store(t).list(), hasLength(1));
    });

    test('siblings, nested paths and unsafe names are skipped', () async {
      const body = '''<D:multistatus xmlns:D="DAV:">
<D:response><D:href>/InfoDesk2/a.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
<D:response><D:href>/InfoDesk/sub/b.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
<D:response><D:href>/InfoDesk/..%2Fevil.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
<D:response><D:href>/InfoDesk/ok.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>''';
      final t = FakeTailnet((_) async => multistatus(body));
      expect((await store(t).list()).map((f) => f.name), ['ok.json']);
    });

    test(
      'only propstat 200 is trusted; a file without a date is skipped',
      () async {
        const body = '''<D:multistatus xmlns:D="DAV:">
<D:response><D:href>/InfoDesk/a.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 404 Not Found</D:status></D:propstat></D:response>
<D:response><D:href>/InfoDesk/b.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</D:getlastmodified></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>''';
        final t = FakeTailnet((_) async => multistatus(body));
        expect((await store(t).list()).map((f) => f.name), ['b.json']);
      },
    );

    test('a non-207 success is an error', () async {
      final t = FakeTailnet((_) async => multistatus(synology, 200));
      await expectLater(
        store(t).list(),
        throwsA(nasError(StorageErrorKind.other)),
      );
    });

    test('a 207 whose root is not multistatus is an error', () async {
      final t = FakeTailnet((_) async => multistatus('<html><body/></html>'));
      await expectLater(
        store(t).list(),
        throwsA(nasError(StorageErrorKind.other)),
      );
    });

    test('malformed xml is an error', () async {
      final t = FakeTailnet((_) async => multistatus('<D:multistatus'));
      await expectLater(
        store(t).list(),
        throwsA(nasError(StorageErrorKind.other)),
      );
    });
  });

  group('errors', () {
    Future<void> expectFor(int code, StorageErrorKind kind, [String? msg]) {
      final t = FakeTailnet((_) async => http.Response('', code));
      return expectLater(
        store(t).list(),
        throwsA(nasError(kind, msg)),
        reason: '$code',
      );
    }

    test('401 is bad credentials', () {
      return expectFor(
        401,
        StorageErrorKind.badCredentials,
        'NAS のユーザー名かパスワードが違います',
      );
    });
    test('403', () {
      return expectFor(403, StorageErrorKind.other, 'NAS のフォルダに書き込む権限がありません');
    });
    test('507 and 413', () async {
      await expectFor(507, StorageErrorKind.other, 'NAS の空き容量が足りません');
      await expectFor(413, StorageErrorKind.other, 'NAS の空き容量が足りません');
    });
    test('redirect', () async {
      for (final c in [301, 302, 307]) {
        await expectFor(
          c,
          StorageErrorKind.other,
          'NAS の URL を確かめてください (転送されました)',
        );
      }
    });
    test('408, 429 and 5xx are retryable', () async {
      for (final c in [408, 429, 500, 503]) {
        await expectFor(c, StorageErrorKind.retryable);
      }
    });
    test('404 is an ordinary error', () {
      return expectFor(404, StorageErrorKind.other);
    });
    test('a transport failure is retryable', () async {
      final t = FakeTailnet((_) async => throw http.ClientException('boom'));
      await expectLater(
        store(t).list(),
        throwsA(nasError(StorageErrorKind.retryable)),
      );
    });
  });

  group('tailnet only', () {
    test('a host outside the tailnet is refused before any request', () async {
      final t = FakeTailnet((_) async => multistatus(synology));
      await expectLater(
        store(t, url: 'http://example.com/InfoDesk/').list(),
        throwsA(
          nasError(
            StorageErrorKind.other,
            'NAS が Tailscale の中に見つかりません。NAS の名前を確かめてください',
          ),
        ),
      );
      expect(t.requests, isEmpty);
      expect(t.clientCalls, 0);
    });

    test('https is refused before any request', () async {
      final t = FakeTailnet((_) async => multistatus(synology));
      await expectLater(
        store(t, url: 'https://nas:5006/InfoDesk/').list(),
        throwsA(
          nasError(
            StorageErrorKind.other,
            'http:// を使ってください (Tailscale の中なので暗号化されています)',
          ),
        ),
      );
      expect(t.requests, isEmpty);
    });

    test('an unparsable URL is refused', () async {
      final t = FakeTailnet((_) async => multistatus(synology));
      for (final u in ['', 'nas', 'ftp://nas/x/', 'http:///x']) {
        await expectLater(
          store(t, url: u).list(),
          throwsA(isA<StorageError>()),
          reason: u,
        );
      }
      expect(t.requests, isEmpty);
    });

    test('create and download are checked too', () async {
      final t = FakeTailnet((_) async => http.Response('', 201));
      final s = store(t, url: 'http://example.com/x/');
      await expectLater(
        s.create('ops-a.json', [1], 'application/json'),
        throwsA(isA<StorageError>()),
      );
      await expectLater(
        s.download(
          const RemoteFile(id: 'a', name: 'a', modifiedTime: '', size: 1),
        ),
        throwsA(isA<StorageError>()),
      );
      expect(t.requests, isEmpty);
    });
  });

  group('create and download', () {
    test('create sends a PUT with Basic auth to the rewritten URL', () async {
      final t = FakeTailnet((req) async {
        if (req.method == 'PUT') return http.Response('', 201);
        return multistatus('''<D:multistatus xmlns:D="DAV:">
<D:response><D:href>/InfoDesk/ops-d1-1.json</D:href>
<D:propstat><D:prop><D:getlastmodified>Thu, 15 Oct 2026 00:00:00 GMT</D:getlastmodified><D:getcontentlength>2</D:getcontentlength></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat></D:response>
</D:multistatus>''');
      });
      final f = await store(
        t,
      ).create('ops-d1-1.json', utf8.encode('[]'), 'application/json');
      final put = t.requests.first;
      expect(put.method, 'PUT');
      expect(
        put.url.toString(),
        'http://100.64.0.7:5005/InfoDesk/ops-d1-1.json',
      );
      expect(
        put.headers['Authorization'],
        'Basic ${base64Encode(utf8.encode('kou:pw'))}',
      );
      expect(put.headers['Content-Type'], startsWith('application/json'));
      expect(put.bodyBytes, utf8.encode('[]'));
      expect(f.name, 'ops-d1-1.json');
      expect(f.modifiedTime, '2026-10-15T00:00:00Z');
      expect(t.requests.last.method, 'PROPFIND');
      expect(t.requests.last.headers['Depth'], '0');
    });

    test('create refuses an unsafe name without any request', () async {
      final t = FakeTailnet((_) async => http.Response('', 201));
      for (final n in ['', '.', '..', 'a/b', r'a\b']) {
        await expectLater(
          store(t).create(n, [1], 'application/json'),
          throwsA(isA<StorageError>()),
          reason: n,
        );
      }
      expect(t.requests, isEmpty);
    });

    test('download GETs the file with Basic auth', () async {
      final t = FakeTailnet((_) async => http.Response.bytes([5, 6, 7], 200));
      final bytes = await store(t).download(
        const RemoteFile(
          id: 'day 1.json.gz',
          name: 'day 1.json.gz',
          modifiedTime: '',
          size: 3,
        ),
      );
      expect(bytes, [5, 6, 7]);
      final r = t.requests.single;
      expect(r.method, 'GET');
      expect(
        r.url.toString(),
        'http://100.64.0.7:5005/InfoDesk/day%201.json.gz',
      );
      expect(r.headers['Authorization'], startsWith('Basic '));
    });

    test('download surfaces 401', () async {
      final t = FakeTailnet((_) async => http.Response('', 401));
      await expectLater(
        store(t).download(
          const RemoteFile(id: 'a', name: 'a', modifiedTime: '', size: 1),
        ),
        throwsA(nasError(StorageErrorKind.badCredentials)),
      );
    });
  });

  group('peerAddrIn', () {
    final peers = [
      const TailnetPeer(
        hostName: 'Takeuchi-NAS',
        dnsName: 'takeuchi-nas.tail1234.ts.net.',
        ips: ['fd7a:115c::7', '100.64.0.7'],
      ),
      const TailnetPeer(
        hostName: 'iphone',
        dnsName: 'iphone.tail1234.ts.net.',
        ips: ['100.64.0.9'],
      ),
    ];
    test('matches HostName, short name, DNSName and IP', () {
      for (final h in [
        'Takeuchi-NAS',
        'takeuchi-nas',
        'takeuchi-nas.tail1234.ts.net',
        'takeuchi-nas.tail1234.ts.net.',
        '100.64.0.7',
      ]) {
        expect(peerAddrIn(peers, h)?.address, '100.64.0.7', reason: h);
      }
    });
    test('prefers IPv4 and finds nothing for strangers', () {
      expect(peerAddrIn(peers, 'iphone')?.address, '100.64.0.9');
      expect(peerAddrIn(peers, 'example.com'), isNull);
      expect(peerAddrIn(peers, ''), isNull);
      expect(peerAddrIn(peers, '8.8.8.8'), isNull);
    });
    test('an exact name beats a short-name match', () {
      final list = [
        const TailnetPeer(
          hostName: 'nas',
          dnsName: 'a.ts.net.',
          ips: ['100.1.1.1'],
        ),
        const TailnetPeer(
          hostName: 'x',
          dnsName: 'nas.ts.net.',
          ips: ['100.2.2.2'],
        ),
      ];
      expect(peerAddrIn(list, 'nas.ts.net')?.address, '100.2.2.2');
    });
  });
}

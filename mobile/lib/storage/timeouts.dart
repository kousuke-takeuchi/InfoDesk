import 'dart:async';

import 'package:http/http.dart' as http;

const defaultConnectTimeout = Duration(seconds: 10);
const defaultRequestTimeout = Duration(seconds: 60);

/// Fails with [TimeoutException] when response headers take longer than
/// [connectTimeout]. The whole-request limit (body read included) is applied
/// by the caller.
class TimeoutClient extends http.BaseClient {
  TimeoutClient(this._inner, this.connectTimeout);

  final http.Client _inner;
  final Duration connectTimeout;

  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) =>
      _inner.send(request).timeout(connectTimeout);

  @override
  void close() => _inner.close();
}

import 'dart:io';

import 'package:http/http.dart' as http;
import 'package:tailscale/tailscale.dart';

/// The part of the in-app Tailscale node the NAS store needs.
abstract class TailnetPort {
  /// An HTTP client whose traffic goes through the tailnet.
  Future<http.Client> client();

  /// The Tailscale address of the peer that [host] names, or null when no peer
  /// matches (the host is then outside the tailnet).
  Future<InternetAddress?> peerAddr(String host);
}

class TailnetPeer {
  const TailnetPeer({
    required this.hostName,
    required this.dnsName,
    required this.ips,
  });

  final String hostName;
  final String dnsName;
  final List<String> ips;
}

/// Finds the peer that [host] names, by HostName, DNSName (trailing dot
/// optional, or the short name before the first dot) or Tailscale IP. Full
/// names and IPs win over short names. IPv4 is preferred.
InternetAddress? peerAddrIn(List<TailnetPeer> peers, String host) {
  var h = host.trim();
  if (h.startsWith('[')) h = h.substring(1);
  if (h.endsWith(']')) h = h.substring(0, h.length - 1);
  while (h.endsWith('.')) {
    h = h.substring(0, h.length - 1);
  }
  if (h.isEmpty) return null;
  final ip = InternetAddress.tryParse(h);
  final lower = h.toLowerCase();

  List<InternetAddress> ipsOf(TailnetPeer p) => [
    for (final s in p.ips) ?InternetAddress.tryParse(s),
  ];

  bool exact(TailnetPeer p) {
    var fqdn = p.dnsName;
    while (fqdn.endsWith('.')) {
      fqdn = fqdn.substring(0, fqdn.length - 1);
    }
    if (fqdn.isNotEmpty && fqdn.toLowerCase() == lower) return true;
    return ip != null && ipsOf(p).any((a) => a.address == ip.address);
  }

  bool short(TailnetPeer p) {
    final s = p.dnsName.split('.').first;
    return (s.isNotEmpty && s.toLowerCase() == lower) ||
        (p.hostName.isNotEmpty && p.hostName.toLowerCase() == lower);
  }

  TailnetPeer? found;
  for (final p in peers) {
    if (exact(p)) {
      found = p;
      break;
    }
  }
  // An IP-shaped host is never compared as a name.
  if (found == null && ip == null) {
    for (final p in peers) {
      if (short(p)) {
        found = p;
        break;
      }
    }
  }
  if (found == null) return null;
  final ips = ipsOf(found);
  for (final a in ips) {
    if (a.type == InternetAddressType.IPv4) return a;
  }
  return ips.isEmpty ? null : ips.first;
}

/// The real thing, over the `tailscale` package's singleton.
class TailscaleTailnetPort implements TailnetPort {
  TailscaleTailnetPort([Tailscale? tailscale])
    : _ts = tailscale ?? Tailscale.instance;

  final Tailscale _ts;

  @override
  Future<http.Client> client() async => _ts.http.client;

  @override
  Future<InternetAddress?> peerAddr(String host) async {
    final nodes = await _ts.nodes();
    return peerAddrIn([
      for (final n in nodes)
        TailnetPeer(
          hostName: n.hostName,
          dnsName: n.dnsName,
          ips: n.tailscaleIPs,
        ),
    ], host);
  }
}

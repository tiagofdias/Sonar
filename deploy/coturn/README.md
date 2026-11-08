# Operator-owned TURN relay

This template is a starting point, not a deployed service. Running coturn on a
machine you already control does not require a paid TURN provider; electricity,
Internet access, public addressing, bandwidth and any hosting can still cost money.

1. Install a supported coturn release on your own host. Copy
   `turnserver.conf.example` outside the repository to a private configuration
   file. Do not commit the file or expose it to web clients.
2. Set `realm` to your relay hostname and `static-auth-secret` to a unique,
   cryptographically random secret. Use the same secret as `TURN_SHARED_SECRET`
   in the backend environment, and set `TURN_URLS` to the reachable relay URL.
   Never use the example placeholder or put this secret in frontend settings.
3. Configure the relay's public address and, when needed, the public/private
   NAT mapping. Open the listener and the selected UDP relay range in both host
   and network firewalls. Review every address reachable by the host, including
   IPv6 and mapped addresses, and block access to internal and sensitive networks.
   The sample IPv4 deny list is not a complete firewall policy.
4. Configure TLS with a valid certificate and a `turns:` URL when transport
   privacy to the relay is required. Plain TURN may expose temporary credentials
   to the network path. WebRTC media uses DTLS-SRTP separately from TURN transport.
5. Start coturn using your operating system's service manager and test from two
   separate networks. First use Sonar's relay connectivity test, then check actual
   audio, disconnection and a call lasting longer than one hour. A successful
   allocation alone does not establish end-to-end voice privacy or long-call reliability.

The template limits allocations and throughput, but is not a monthly traffic cap
and cannot guarantee a zero-cost Internet connection. Configure any provider's
hard usage limits separately, with paid upgrades disabled. For options and version
differences, use the [official coturn configuration reference](https://github.com/coturn/coturn/blob/master/examples/etc/turnserver.conf).

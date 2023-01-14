# dyndns

Dynamically configurable recursive dns resolver

This server exposes an HTTP API with which records can be dynamically added after runtime. The only configuration the server needs at startup (though this configuration can be _changed_ after start) is the upstream DNS IP address to use (defaults to 1.1.1.1).

```
# dyndns --upstream-ip 1.1.1.1
dyndns config listening on 127.0.0.1:6060...
```

*While the server is running*

```
$ curl -X POST http://localhost:6060 -d '{"operation": "add", "payload": {"type": "a", "hostname": "exmaple.com", "ip-address": "127.0.0.1"}}'

$ dig +noall +answer example.com
example.com.            85176   IN      A       127.0.0.1
...


```

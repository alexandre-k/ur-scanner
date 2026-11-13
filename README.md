# How to deploy

```bash
podman build -t ur-scanner:latest .
podman run --rm ur-scanner:latest
podman tag ur-scanner:latest xyz/ur-scanner:latest
podman push xyz/ur-scanner:latest
```

Helm chart:

```bash
helm upgrade --install ur-scanner .   -n ur-scanner   --create-namespace   -f values.yaml
```

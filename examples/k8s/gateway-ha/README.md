# Gateway HA — Kubernetes

Minimal manifests for a 2-replica `dcc-mcp-server` gateway deployment
sharing a `ReadWriteMany` `FileRegistry` volume. Deliberately **not** a
Helm chart — copy, tweak, apply.

Files:

- `configmap.yaml` — environment for all replicas.
- `deployment.yaml` — Deployment + RWX PVC.
- `service.yaml` — ClusterIP service on port `9765`.

See [`docs/guide/production-deployment.md`](../../../docs/guide/production-deployment.md)
for the full topology and failover semantics.

## Prerequisites

- A Kubernetes cluster with a `ReadWriteMany` storage class (EFS,
  CephFS, Longhorn RWX, …). Edit `deployment.yaml` to set
  `storageClassName` to the class name in your cluster.
- The container image — build it from
  `examples/compose/gateway-ha/Dockerfile` and push to a registry your
  cluster can pull from. Update the `image:` field in `deployment.yaml`.

## Apply

```bash
kubectl apply -f examples/k8s/gateway-ha/
kubectl rollout status deploy/dcc-mcp-gateway
kubectl get pods -l app.kubernetes.io/name=dcc-mcp-gateway
```

Dry-run validation before applying:

```bash
kubectl apply --dry-run=client -f examples/k8s/gateway-ha/
```

## Smoke test

```bash
# Port-forward the service
kubectl port-forward svc/dcc-mcp-gateway 9765:9765 &
PF_PID=$!

# Health
curl -sf http://localhost:9765/health
# → {"ok":true}

# Registered instances
curl -s http://localhost:9765/instances | jq

# MCP — list aggregated tools
curl -sf -X POST http://localhost:9765/mcp \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' | jq '.result.tools | length'

kill "$PF_PID"
```

## Failover test

```bash
# Delete one pod; replicas=2 and maxUnavailable=0 ensure the service stays up.
POD=$(kubectl get pod -l app.kubernetes.io/name=dcc-mcp-gateway -o name | head -n1)
kubectl delete "$POD"
sleep 6
kubectl port-forward svc/dcc-mcp-gateway 9765:9765 &
curl -sf http://localhost:9765/health    # still 200
kill %1
```

## Ingress / session stickiness

A plain ClusterIP is enough for in-cluster clients. For external clients
add an Ingress (nginx-ingress example):

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: dcc-mcp-gateway
  annotations:
    nginx.ingress.kubernetes.io/upstream-hash-by: "$http_mcp_session_id"
    nginx.ingress.kubernetes.io/proxy-buffering: "off"
    nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"
spec:
  ingressClassName: nginx
  tls:
    - hosts: [mcp.example.com]
      secretName: dcc-mcp-tls
  rules:
    - host: mcp.example.com
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: dcc-mcp-gateway
                port: { number: 9765 }
```

## Tear down

```bash
kubectl delete -f examples/k8s/gateway-ha/
```

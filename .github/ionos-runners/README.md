# IONOS runners for ci.yml

`ci.yml` runs on two [ARC](https://github.com/actions/actions-runner-controller)
runner scale sets on swack-tools' IONOS Managed Kubernetes cluster (`ionos`,
us/las), registered to the swack-tools org:

| Label | Pods per node | Requests (CPU / memory) | Jobs |
|---|---|---|---|
| `ionos-large` | 1 | 12 / 11.5 GiB | Build & Test, Release Build, capture, Benchmarks |
| `ionos-small` | 3 | 4 / 4 GiB | everything else |

CPU is a request, never a limit, so a job can use a node's idle threads.
Memory limit equals the request. Each node keeps about 2 CPU and 2.5 GiB
unrequested for cluster services (Prometheus). Pods carry
`cluster-autoscaler.kubernetes.io/safe-to-evict: "false"` so the autoscaler
never removes a node under a running job.

## Node pool

`ci-runners`: 8 cores (16 threads), 16 GB RAM, 100 GB SSD, label
`workload=ci-runners`, autoscaling from 1 node up to what the contract's core
and RAM limits allow. Every pod here (controller, listeners, runners, pre-pull)
selects that label. There are no warm spares: jobs beyond the free capacity
wait for the autoscaler to add a node.

## Files

- `image/Dockerfile`: `ghcr.io/swack-tools/ci-runner`, the ARC runner image
  with oxidex's build tools, Archive::Zip and Rust 1.97.1 preinstalled.
- `controller-values.yaml`: gha-runner-scale-set-controller values.
- `ionos-large.yaml`, `ionos-small.yaml`: gha-runner-scale-set values.
- `prepull.yaml`: DaemonSet that pulls the runner image onto each new node.
- `bench/`: the two single-node scale sets used to choose the node type
  (branch `bench/runner-comparison`).

## Install

```bash
export KUBECONFIG=~/.kube/ionos.yaml
CHART=oci://ghcr.io/actions/actions-runner-controller-charts
helm upgrade --install arc -n arc-systems --create-namespace --version 0.14.2 \
  -f controller-values.yaml $CHART/gha-runner-scale-set-controller
kubectl create namespace arc-runners
# Org credential (admin:org), never echoed:
gh auth token -u swackhamer | tr -d '\n' | kubectl -n arc-runners \
  create secret generic arc-github --from-file=github_token=/dev/stdin
for s in ionos-large ionos-small; do
  helm upgrade --install $s -n arc-runners --version 0.14.2 -f $s.yaml \
    $CHART/gha-runner-scale-set
done
kubectl apply -f prepull.yaml
```

While the ghcr package is private, the pods also need the `ghcr-pull`
docker-registry secret in `arc-runners` (`imagePullSecrets`); once the package
is public that secret and those references can go.

## Rebuilding the image

```bash
docker buildx build --platform linux/amd64 \
  -t ghcr.io/swack-tools/ci-runner:<runner-version>-<n> --push image
```

Then bump the tag in `ionos-large.yaml`, `ionos-small.yaml` and `prepull.yaml`
and re-run the helm upgrades.

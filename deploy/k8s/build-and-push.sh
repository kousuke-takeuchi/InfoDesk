#!/usr/bin/env bash
# Builds the image on the WSL node of 192.168.0.175 and pushes it to registry.umafolio.site.
# Usage: deploy/k8s/build-and-push.sh   (prints the image tag)
set -euo pipefail
host="Kousuke Takeuchi@192.168.0.175"
tag="$(git rev-parse --short HEAD)"
image="registry.umafolio.site/infodesk:${tag}"
dir="/root/build/infodesk-${tag}"
# base64 keeps the tar intact through the Windows PowerShell session.
git archive --format=tar.gz HEAD | base64 | ssh -o BatchMode=yes "$host" \
  "wsl -d Ubuntu-24.04 -- bash -c 'rm -rf ${dir} && mkdir -p ${dir} && base64 -d | tar -xz -C ${dir}'"
ssh -o BatchMode=yes "$host" "wsl -d Ubuntu-24.04 -- bash -c 'cd ${dir} && docker build --target test -t infodesk-test:${tag} . && docker build -t ${image} . && docker push ${image} && rm -rf ${dir}'"
echo "$image"

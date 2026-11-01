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
# The build directory is removed whether the build succeeds or fails; the exit status is kept.
# \$ stays literal here, and PowerShell does not expand it inside single quotes.
ssh -o BatchMode=yes "$host" "wsl -d Ubuntu-24.04 -- bash -c 'cd ${dir} && docker build --target test -t infodesk-test:${tag} . && docker build -t ${image} . && docker push ${image}; status=\$?; rm -rf ${dir}; exit \$status'"
echo "$image"

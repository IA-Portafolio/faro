#!/usr/bin/env bash
# Instala un GitHub Actions self-hosted runner en infra-iaportafolio.
# Idempotente: si ya está instalado, solo verifica el estado.
#
# Uso:
#   REG_TOKEN="$(saca el token de Settings → Actions → Runners → New self-hosted runner)" \
#   bash runner-install.sh

set -euo pipefail

REPO_URL="https://github.com/IA-Portafolio/faro"
RUNNER_USER="${RUNNER_USER:-victalejo}"
RUNNER_DIR="${RUNNER_DIR:-/opt/actions-runner}"
RUNNER_NAME="${RUNNER_NAME:-faro-infra}"
RUNNER_LABELS="${RUNNER_LABELS:-self-hosted,linux,faro-deploy}"
RUNNER_VERSION="${RUNNER_VERSION:-2.319.1}"
RUNNER_ARCH="${RUNNER_ARCH:-x64}"

: "${REG_TOKEN:?Define REG_TOKEN con el token de registro de GitHub}"

if [ ! -d "$RUNNER_DIR" ]; then
    echo "==> Creando $RUNNER_DIR"
    sudo mkdir -p "$RUNNER_DIR"
    sudo chown "$RUNNER_USER:$RUNNER_USER" "$RUNNER_DIR"
fi

cd "$RUNNER_DIR"

if [ ! -f ./config.sh ]; then
    echo "==> Descargando runner v$RUNNER_VERSION"
    sudo -u "$RUNNER_USER" curl -fsSL -o runner.tar.gz \
        "https://github.com/actions/runner/releases/download/v${RUNNER_VERSION}/actions-runner-linux-${RUNNER_ARCH}-${RUNNER_VERSION}.tar.gz"
    sudo -u "$RUNNER_USER" tar xzf runner.tar.gz
    sudo -u "$RUNNER_USER" rm runner.tar.gz
fi

# Dependencias del runner (Node embebido + libs)
sudo ./bin/installdependencies.sh || true

if [ ! -f .runner ]; then
    echo "==> Registrando runner en $REPO_URL"
    sudo -u "$RUNNER_USER" ./config.sh \
        --unattended \
        --url "$REPO_URL" \
        --token "$REG_TOKEN" \
        --name "$RUNNER_NAME" \
        --labels "$RUNNER_LABELS" \
        --work _work \
        --replace
else
    echo "==> Runner ya estaba registrado, saltando config"
fi

# Asegurar que el usuario puede usar Docker (deploy.yml lo necesita).
if ! id -nG "$RUNNER_USER" | grep -q docker; then
    echo "==> Añadiendo $RUNNER_USER al grupo docker"
    sudo usermod -aG docker "$RUNNER_USER"
fi

# Permitir al runner escribir en /opt/faro (lo hace rsync).
sudo chown -R "$RUNNER_USER:$RUNNER_USER" /opt/faro || true

# Instalar como servicio systemd para que sobreviva reinicios.
if [ ! -f /etc/systemd/system/actions.runner.IA-Portafolio-faro.${RUNNER_NAME}.service ]; then
    echo "==> Instalando como servicio systemd"
    sudo ./svc.sh install "$RUNNER_USER"
fi

sudo ./svc.sh start

echo ""
echo "✓ Runner instalado y corriendo."
sudo ./svc.sh status | tail -5

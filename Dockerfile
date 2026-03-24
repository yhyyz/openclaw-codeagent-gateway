FROM node:22-slim

ARG AGW_VERSION=v0.1.0
ARG AGW_REPO=yhyyz/openclaw-codeagent-gateway

# System dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates git && \
    rm -rf /var/lib/apt/lists/*

# Install AI coding agent CLI tools
RUN npm install -g opencode-ai && \
    npm cache clean --force
# npx auto-downloads @zed-industries/claude-agent-acp on first use
# kiro-cli requires separate installation — see README

# Download agw binary from GitHub release
RUN curl -L "https://github.com/${AGW_REPO}/releases/download/${AGW_VERSION}/agw-linux-x86_64.tar.gz" \
    -o /tmp/agw.tar.gz && \
    tar xzf /tmp/agw.tar.gz -C /tmp && \
    mv /tmp/agw-linux-x86_64 /usr/local/bin/agw && \
    chmod +x /usr/local/bin/agw && \
    rm -rf /tmp/agw*

# Create directories
RUN mkdir -p /data /workspace /root/.config/opencode

EXPOSE 8001
VOLUME ["/data", "/workspace"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8001/health || exit 1

ENTRYPOINT ["agw", "serve"]
CMD ["--config", "/etc/agw/gateway.yaml"]

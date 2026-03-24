FROM node:22-slim

# System dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates git && \
    rm -rf /var/lib/apt/lists/*

# Install AI coding agent CLI tools
RUN npm install -g opencode-ai && \
    npm cache clean --force

# npx will auto-download @zed-industries/claude-agent-acp on first use
# kiro-cli requires separate installation — see README

# Copy agw binary (pre-built for linux x86_64)
COPY target/release/agw /usr/local/bin/agw
RUN chmod +x /usr/local/bin/agw

# Copy default config
COPY gateway.yaml.example /etc/agw/gateway.yaml.example

# Create data and workspace directories
RUN mkdir -p /data /workspace

# Expose HTTP API port
EXPOSE 8001

# Volumes for persistent data and workspace
VOLUME ["/data", "/workspace"]

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8001/health || exit 1

ENTRYPOINT ["agw", "serve"]
CMD ["--config", "/etc/agw/gateway.yaml"]

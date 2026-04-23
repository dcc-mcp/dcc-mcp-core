FROM python:3.11-slim

WORKDIR /app

RUN pip install --no-cache-dir dcc-mcp-core

COPY server.py .
COPY skills/ /opt/skills/

ENV DCC_MCP_HOST=0.0.0.0
ENV DCC_MCP_PORT=8765
ENV DCC_MCP_SKILL_PATHS=/opt/skills
# Set DCC_MCP_API_KEY at runtime: docker run -e DCC_MCP_API_KEY=secret ...

EXPOSE 8765

CMD ["python", "server.py"]

FROM python:3.14-slim

WORKDIR /app

RUN pip install --no-cache-dir dcc-mcp-core

COPY server.py .
COPY skills/ ./skills/

ENV DCC_MCP_HOST=0.0.0.0
ENV DCC_MCP_PORT=8765

EXPOSE 8765

CMD ["python", "server.py"]

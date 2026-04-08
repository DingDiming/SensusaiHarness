FROM python:3.12-slim

WORKDIR /workspace

# Install system deps + Node.js (for codex CLI)
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - && \
    apt-get install -y nodejs && rm -rf /var/lib/apt/lists/*

# Install Codex CLI globally
RUN npm install -g @openai/codex@latest

# Install uv for fast Python installs
RUN pip install --no-cache-dir uv

COPY pyproject.toml ./backend/
RUN cd backend && uv pip install --system --no-cache -e "."

COPY . ./backend/

EXPOSE 8000

CMD ["uvicorn", "backend.src.main:app", "--host", "0.0.0.0", "--port", "8000"]

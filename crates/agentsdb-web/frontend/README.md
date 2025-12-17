# AGENTS.db Web Frontend

Modern web UI for AGENTS.db built with:

- **Preact** - Lightweight React alternative
- **TypeScript** - Type safety
- **Vite** - Fast build tool
- **Tailwind CSS** - Utility-first CSS
- **DaisyUI** - Component library for Tailwind

## Development

Install dependencies:

```bash
npm install
```

Start the development server (runs on http://localhost:5173):

```bash
npm run dev
```

The dev server proxies API requests to the Rust backend on http://localhost:9090.

## Building

Build for production:

```bash
npm run build
```

This outputs the built assets to `../dist/` which the Rust server will serve.

## Type Checking

Run TypeScript type checking:

```bash
npm run type-check
```

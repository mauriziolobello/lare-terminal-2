"""Server MCP di prova per l'infrastruttura pytools — vedi
Docs/superpowers/specs/2026-07-21-pytools-infrastructure-design.md.

Un solo tool, pyping, che restituisce un'eco deterministica del messaggio
ricevuto. Isola i bug di meccanismo (venv, spawn, handshake MCP) da quelli
di dominio, prima che arrivi un tool Python reale (es. investpy).

L'orchestratore (vedi `crates/orchestrator/src/python_mcp_tool_client.rs`,
`ensure_connected`) spawna OGNI server MCP Python con `--config-dir <dir>`
come argomento, uniformemente, che il dominio ne abbia bisogno o no: questo
server accetta `--config-dir` per uniformità, non lo usa (nessuna
configurazione da leggere per un semplice eco). FastMCP non tocca
`sys.argv` in `run()`, quindi l'argomento extra non causa mai un errore
"unrecognized arguments" -- non serve un parser qui.
"""

from mcp.server.fastmcp import FastMCP

mcp = FastMCP("python-ping")


@mcp.tool()
def pyping(message: str) -> str:
    """Restituisce un'eco del messaggio ricevuto, prefissata da 'pong: '."""
    return f"pong: {message}"


if __name__ == "__main__":
    mcp.run(transport="stdio")

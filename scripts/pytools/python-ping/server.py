"""Server MCP di prova per l'infrastruttura pytools — vedi
Docs/superpowers/specs/2026-07-21-pytools-infrastructure-design.md.

Un solo tool, pyping, che restituisce un'eco deterministica del messaggio
ricevuto. Isola i bug di meccanismo (venv, spawn, handshake MCP) da quelli
di dominio, prima che arrivi un tool Python reale (es. investpy).
"""

from mcp.server.fastmcp import FastMCP

mcp = FastMCP("python-ping")


@mcp.tool()
def pyping(message: str) -> str:
    """Restituisce un'eco del messaggio ricevuto, prefissata da 'pong: '."""
    return f"pong: {message}"


if __name__ == "__main__":
    mcp.run(transport="stdio")

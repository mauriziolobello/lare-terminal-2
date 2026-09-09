# Lare — Comandos

Escribe los comandos `/…` en la línea de comandos de Lare Terminal (tu sesión de PowerShell).
El resultado de cada comando slash aparece en una ventana; en el terminal permanece una línea de confirmación.

## AI
- `/ai "solicitud"` o `/ "solicitud"` — la IA responde y ejecuta comandos **en tu shell**
  (cada comando propuesto pide confirmación `[Y/n]` antes de ejecutarse). Las comillas son obligatorias.

## Comandos
- `/help` — esta ventana.
- `/ping` — verifica las capas de Lare (lare-shell, orchestrator, plugin-ping, ui.exe).
- `/config` — configuración (apariencia, búsqueda web, IA, mercados).
- `/library` — archivo de documentos guardados (reabribles).
- `/aichat` — AI Chat (comunicación entre máquinas Lare en red, con participación de la IA).
- `/open <target>` — abre una URL, carpeta o archivo con la aplicación predeterminada. Sin
  comillas alrededor del target (a diferencia de `/ai` más arriba): escríbelo tal cual, incluso
  con espacios.
- `/web <query>` — busca la consulta en el navegador predeterminado.
- `/show <markdown>` — abre una ventana con el Markdown indicado.
- `/find [<query>] [in:"<frase>"] [folder:from-here]` — búsqueda de archivos en vivo, ventana dedicada.
- `/reset` — reinicia la sesión de shell.
- `/nowin <solicitud>` — la IA responde como texto en el terminal, sin ventana Markdown.
- `/calc` — calculadora (plugin).

## Herramientas externas (ventana dedicada)
- `/markets` — herramientas sobre mercados financieros (búsqueda de ticker, informe bursátil, lista de valores, screener).
- `/nmap` — herramientas de escaneo de red (escaneo rápido, detección de SO/versiones, descubrimiento de hosts, búsqueda de vulnerabilidades).
- `/pyping` — canal de prueba para la infraestructura de herramientas Python (eco de un mensaje).

## Todo lo demás
- Cualquier línea que no empiece con `/` es PowerShell, como siempre.
- Un comando slash desconocido se ignora en silencio.

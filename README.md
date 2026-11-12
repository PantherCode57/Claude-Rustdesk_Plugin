# RustDesk MCP para Claude

Permite que Claude maneje **otro ordenador** por RustDesk sin ocupar tu pantalla, tu ratón ni tu
teclado. Tú sigues trabajando mientras Claude hace la tarea en el equipo remoto.

Qué trae:

- **Servidor MCP `rustdesk`**: herramientas para conectar, abrir programas por nombre, pulsar
  atajos de teclado, escribir, hacer clic por nombre o por coordenadas y mirar la pantalla.
- **Skill `/rusdesk`**: cuando pides abrir o controlar otro ordenador, Claude usa estas
  herramientas en lugar de tomar el control de tu pantalla.
- **Mini visor**: en la app de escritorio de Claude, una franja encima de la barra de escribir
  con la pantalla remota en directo, el puntero de Claude y la lista de tareas.
- **Puente** (`bin/rustdesk-claude.exe`): un cliente de RustDesk sin ventana. Solo Windows.

Claude trabaja primero por texto: abre programas por nombre, usa atajos y pulsa por nombre los
sitios que ya conoce. Solo mira la pantalla cuando es estrictamente necesario, y cada sitio
nuevo que pulsa se apunta solo en el mapa de ese ordenador para la próxima vez.

## Instalar

En una terminal:

```
claude plugin install rustdesk-mcp --marketplace PantherCode57/rustdesk-mcp
```

O dentro de una sesión de Claude Code en terminal:

```
/plugin install rustdesk-mcp --marketplace PantherCode57/rustdesk-mcp
```

En la app de escritorio: pulsa + junto a la barra de escribir, Plugins, Add marketplace con
`PantherCode57/rustdesk-mcp` y después Add plugin.

El repositorio es privado: solo se instala en equipos donde GitHub ya esté configurado con una
cuenta que tenga acceso (`gh auth login` y `gh auth setup-git`).

Requisitos: Windows y `python` en el PATH.

## Configurar los ordenadores

La primera vez se crea `C:\RustDeskClaude\peers.json`. Pon ahí el ID de RustDesk y la contraseña
permanente de cada ordenador, con el nombre que quieras:

```json
{
  "oficina": { "id": "123456789", "password": "la-contraseña" }
}
```

La contraseña se queda solo en ese archivo. No se escribe en el chat ni se sube a GitHub.

## Usar

Escribe `/rusdesk` o pídelo con tus palabras: "conéctate a oficina y abre el Bloc de notas".

## Dónde está cada cosa

| Qué | Dónde |
|---|---|
| Ordenadores y contraseñas | `C:\RustDeskClaude\peers.json` |
| Mapa aprendido de cada ordenador | `C:\RustDeskClaude\maps\<nombre>.json` |
| Atajos de teclado y nombres de programas | `maps/_atajos.json` |
| Visor en directo en el navegador | `http://127.0.0.1:21500/` |
| Código del puente y cómo compilarlo | `bridge/` |

## Límites

- El puente es solo para Windows.
- En el ordenador remoto Windows tiene un único puntero: quien esté delante verá moverse su ratón.
- No se puede enviar Ctrl+Alt+Supr.
- El mini visor es para mirar, no para hacer clic.

## Licencia

El puente se compila a partir de [RustDesk](https://github.com/rustdesk/rustdesk), que es
AGPL-3.0; los cambios están en `bridge/`.

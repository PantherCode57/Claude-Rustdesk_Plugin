---
name: rusdesk
description: Connect to and control ANOTHER computer through RustDesk (the user may write "rusdesk", "rustdesk", "escritorio remoto", "el otro ordenador", "el PC de la oficina", "conéctate a...", "entra en el ordenador de...", "abre RustDesk"). Use this whenever the user wants something opened, checked, typed, clicked or done on a remote PC, INSTEAD of computer use, screenshots of the local screen, or opening the RustDesk app window - it drives the remote PC through the rustdesk_* tools without touching the user's own screen, mouse or keyboard.
---

# RustDesk remote control

The user wants something done on another computer. Do it with the `rustdesk_*` tools of this
plugin's `rustdesk` MCP server. They run a RustDesk client with no window, so the user keeps
their own screen, mouse and keyboard while you work.

Do not use computer use, do not screenshot the local desktop, and do not open the RustDesk
application: those take over the user's screen, which is exactly what this skill exists to avoid.
If the `rustdesk_*` tools are not available, say so and stop; do not fall back to the screen.

## Starting

When the user types `/rusdesk` with nothing after it, the plugin opens a picker in the band
above the prompt (Claude desktop app and terminal): the computers already saved, each one a
button, plus "Agregar nuevo", a form with an ID field and a password field. The form saves to
the peers file by itself; you never see the password. When the user picks or saves a computer,
a message arrives asking you to connect to it by name.

1. The user typed only `/rusdesk` and named no computer and no task: answer in one short line
   that they can choose the computer in the box above the prompt, and stop. Do not call any
   tool, do not ask for the ID or the password, do not explain the peers file.
2. The user named a computer or a task: `rustdesk_status` lists the saved computers by name (a
   computer added through the form is named by its ID). One listed: use it. Several, and the
   user did not say which: ask them to type `/rusdesk` and pick it.
3. `rustdesk_connect` with that name. Its answer is text: the screen size, the places already
   known on that PC and its tips. Read them; they are what lets you work without looking.
4. No computer saved, or the connection fails for a missing or wrong password: ask the user to
   type `/rusdesk` and fill in "Agregar nuevo" (saving the same ID again replaces its
   password). Where the band is not drawn (mobile, other clients), the ID and permanent
   password go in the peers file, written by the user: give the path from `rustdesk_status` and
   the format (`{ "oficina": { "id": "123456789", "password": "..." } }`). Never ask for the
   password in chat, never read the peers file, never write the password yourself.

## Working: text first, pictures last

A screenshot is the slow and costly part. Reach for things in this order:

1. `rustdesk_open` to open a program, folder, settings page or web address by name.
2. `rustdesk_key` for a shortcut (`ctrl+p` print, `ctrl+s` save, `alt+f4` close, `win+e` files...).
   `rustdesk_map` has the full list of shortcuts and program names; read it once per session.
3. `rustdesk_steps` when you already know the sequence: several opens, keys, texts, clicks by
   name and waits in ONE call.
4. `rustdesk_click` with a `name` from the map: no screenshot needed.
5. `rustdesk_screenshot` only when strictly necessary:
   - you do not know what is on screen and the next step depends on it;
   - you need a button or field that is not in the map and has no shortcut;
   - something did not behave as expected;
   - the result matters and has to be checked before you tell the user it is done.
   Use `size: "small"` when you only need to see which window is in front.

After a screenshot, a click by coordinates must carry a `label` (and the `app`): it is written
to that PC's map automatically, so the next time you click it by name. If a click by name turns
out to have missed, call `rustdesk_map_wrong` so the map does not keep a wrong place.

When you learn something that will save time on the next visit (a shortcut that works in one of
its programs, where a file lives, a quirk), store it with `rustdesk_map_tip`.

Actions answer with a line of text. Pass `screenshot: true` on an action only when you would
otherwise have to take a screenshot right after it anyway.

## The user is watching

In the Claude desktop app a band above the prompt shows the remote screen live, your pointer as
an orange arrow, and your task list. Keep a short task list for anything with more than one step
so they can follow what is done, what you are doing and what is left. Anywhere else, the live
view is the page `http://127.0.0.1:21500/` on the user's machine.

## It is someone's real computer

- Leave things as you found them: close what you opened, do not close what was already open.
- Do not type into documents or tabs that were already there; open a new one.
- Do not delete, send, buy, shut down, lock or change settings unless the user asked for exactly that.
- On cookie or consent banners choose the option that shares the least.
- Tell the user plainly what you did, what you did not do and what you could not verify.
- Stay connected while the conversation is about that PC; `rustdesk_disconnect` when the user is done.

## When it does not work

- "rustdesk-claude.exe not found": the plugin's `bin` folder is missing the bridge; it is
  Windows-only and is built from the patch in the plugin's `bridge` folder.
- "Wrong password" / "No password": the user saves it again with `/rusdesk` (see Starting).
- The MCP server does not start: it needs `python` on the PATH.

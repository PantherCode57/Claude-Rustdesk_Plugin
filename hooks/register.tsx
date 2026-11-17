import { atom, read, update } from 'claude-code'
import type { Register } from 'claude-code'

import type { VisorPeer, VisorPicker, VisorStep } from '../types'

const MAX_ROWS = 8
const DEFAULT_HOME = 'C:\\RustDeskClaude'

const svg = atom({ plugin: 'rustdesk-mcp', key: 'svg' } as const, '')
const isHidden = atom({ plugin: 'rustdesk-mcp', key: 'isHidden' } as const, false)
const plan = atom({ plugin: 'rustdesk-mcp', key: 'plan' } as const, [] as VisorStep[])
const activity = atom({ plugin: 'rustdesk-mcp', key: 'activity' } as const, '')
const picker = atom({ plugin: 'rustdesk-mcp', key: 'picker' } as const, 'closed' as VisorPicker)
const peers = atom({ plugin: 'rustdesk-mcp', key: 'peers' } as const, [] as VisorPeer[])
const notice = atom({ plugin: 'rustdesk-mcp', key: 'notice' } as const, '')
// The port of this conversation's own bridge, learned from its rustdesk tools' answers; 0 until
// this conversation connects. Another conversation's remote session is never drawn here.
const port = atom({ plugin: 'rustdesk-mcp', key: 'port' } as const, 0)

type Args = Record<string, unknown>
type PeersFile = Record<string, { id?: string; password?: string }>

const digits = (text: string) => text.replace(/\D/g, '')
const spaced = (id: string) => id.replace(/(\d{3})(?=\d)/g, '$1 ')

const toSteps = (items: unknown): VisorStep[] =>
  (items as Args[]).map(i => ({ content: String(i.content ?? ''), status: String(i.status ?? 'pending') }))

// The task list follows Claude's own plan tool: the whole list, or a step forward.
function nextPlan(steps: VisorStep[], a: Args): VisorStep[] {
  if (Array.isArray(a.items)) return toSteps(a.items)
  if (Array.isArray(a.todos)) return toSteps(a.todos)

  const out = steps.map(s => ({ ...s }))
  const set = (step: VisorStep | undefined, status: string) => {
    if (step !== undefined) step.status = status
  }

  if (a.advance === true) {
    set(out.find(s => s.status === 'in_progress'), 'completed')
    set(out.find(s => s.status === 'pending'), 'in_progress')
  }

  for (const n of Array.isArray(a.done) ? (a.done as number[]) : []) {
    set(out[n - 1], 'completed')
  }

  if (typeof a.start === 'number') set(out[a.start - 1], 'in_progress')

  return out
}

// What Claude is doing on the remote PC right now, from the rustdesk tool it called.
function describe(action: string, a: Args): string | undefined {
  const where = a.name ?? a.label ?? (a.x !== undefined ? `${a.x}, ${a.y}` : '')
  const text = String(a.text ?? '')

  switch (action) {
    case 'connect':
      return `Conectando a ${a.peer}`
    case 'disconnect':
      return 'Desconectando'
    case 'screenshot':
      return 'Mirando la pantalla'
    case 'click':
      return `${a.double === true ? 'Doble clic' : 'Clic'} en ${where}`
    case 'move':
      return `Moviendo el ratón a ${where}`
    case 'drag':
      return 'Arrastrando'
    case 'scroll':
      return 'Desplazando con la rueda'
    case 'type':
      return `Escribiendo "${text.length > 30 ? `${text.slice(0, 30)}…` : text}"`
    case 'key':
      return `Pulsando ${a.key}`
    case 'open':
      return `Abriendo ${a.program}`
    case 'steps':
      return `Ejecutando ${Array.isArray(a.steps) ? a.steps.length : 0} pasos seguidos`
    case 'map':
      return 'Consultando el mapa'
    default:
      return undefined
  }
}

export const register: Register = on => {
  let last = ''
  // The form's text stays in this module only: the password goes from the field to the peers
  // file, never into the session's state, the transcript or anything the model reads.
  let draftId = ''
  let draftPassword = ''

  on('session.start', async ($, e, next) => {
    $.clock.every(1000, async () => {
      // The remote screen as an SVG (a small JPEG inside it, Claude's pointer drawn over it).
      // Empty while this conversation has no remote session: then the band is not drawn.
      let frame = ''
      try {
        const own = await read($, port)
        if (own !== 0) {
          const r = await $.http.fetch(`http://127.0.0.1:${own}/frame.svg?max=640`)
          frame = r.ok ? r.text : ''
        }
      } catch {
        frame = ''
      }
      if (frame !== last) {
        last = frame
        await update($, svg, () => frame)
      }
    })

    return next(e)
  })

  // "/rusdesk" with nothing after it opens the picker: the computers already saved, or the
  // form for a new one.
  on('prompt.submit', async ($, e, next) => {
    try {
      if (e.command !== undefined && /(^|:)rus?t?desk$/i.test(e.command.name) && e.command.args.trim() === '') {
        const home = (await $.env.get('RUSTDESK_CLAUDE_HOME')) ?? DEFAULT_HOME
        let file: PeersFile = {}
        try {
          file = JSON.parse(await $.fs.read(`${home}/peers.json`)) as PeersFile
        } catch {
          file = {}
        }
        // Names and IDs only: the passwords stay in the file.
        const list = Object.entries(file)
          .map(([name, p]) => ({ name, id: digits(String(p?.id ?? '')) }))
          .filter(p => p.id !== '')

        draftId = ''
        draftPassword = ''
        await update($, peers, () => list)
        await update($, notice, () => '')
        await update($, picker, () => (list.length > 0 ? 'list' : 'new'))
      }
    } catch {
      // the picker is a convenience; the prompt goes through either way
    }

    return next(e)
  }).catch(($, e, next) => next(e))

  on('tool.call', async ($, e, next) => {
    const tool: string = e.tool
    const a = e as unknown as Args
    // The tool's own name is what follows the last "__": the server part before it also holds
    // "rustdesk_", so matching on the whole name would take the wrong piece.
    const remote = /^rustdesk_([a-z_]+)$/.exec(tool.slice(tool.lastIndexOf('__') + 2))

    // Watching only: whatever goes wrong here must never stop the tool call itself.
    try {
      if (tool.endsWith('update_plan') || tool === 'TodoWrite') {
        await update($, plan, steps => nextPlan(steps, a))
      } else if (tool.endsWith('wrap_up')) {
        await update($, plan, steps =>
          steps.map(s => (s.status === 'in_progress' ? { ...s, status: 'completed' } : s)),
        )
        await update($, activity, () => '')
      } else if (remote !== null && remote[1] !== undefined) {
        const doing = describe(remote[1], a)
        if (doing !== undefined) await update($, activity, () => doing)
      }
    } catch {
      // the list is a convenience; a missed update is not worth a failed tool call
    }

    const result = await next(e)

    try {
      if (remote !== null && (remote[1] === 'connect' || remote[1] === 'status')) {
        const found = /127\.0\.0\.1:(\d+)/.exec(JSON.stringify(result))
        if (found !== null && found[1] !== undefined) {
          const own = Number(found[1])
          await update($, port, () => own)
        }
      } else if (remote !== null && remote[1] === 'disconnect') {
        await update($, port, () => 0)
      }
    } catch {
      // same: the band is a convenience
    }

    return result
  }).catch(($, e, next) => next(e))

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    if (e.surface !== 'desktop' && e.surface !== 'terminal') {
      return next(e)
    }

    const frame = await read($, svg)
    const mode = await read($, picker)
    const { Box, Button, Input, Text } = $.ui.resolve(e)

    const connect = async (name: string) => {
      await update($, picker, () => 'closed' as VisorPicker)
      await update($, activity, () => `Conectando a ${name}`)
      void $.prompt.submit({ text: `Conéctate por RustDesk al equipo "${name}".` })
    }

    const save = async () => {
      const id = digits(draftId)
      if (id.length < 6) {
        await update($, notice, () => 'Escribe el ID del otro ordenador (solo números).')
        return
      }
      if (draftPassword === '') {
        await update($, notice, () => 'Escribe la contraseña permanente del otro ordenador.')
        return
      }
      try {
        const home = (await $.env.get('RUSTDESK_CLAUDE_HOME')) ?? DEFAULT_HOME
        let file: PeersFile = {}
        try {
          file = JSON.parse(await $.fs.read(`${home}/peers.json`)) as PeersFile
        } catch {
          file = {}
        }
        const name = Object.keys(file).find(n => digits(String(file[n]?.id ?? '')) === id) ?? id
        file[name] = { id, password: draftPassword }
        await $.fs.write(`${home}/peers.json`, `${JSON.stringify(file, null, 2)}\n`)
        draftId = ''
        draftPassword = ''
        await connect(name)
      } catch {
        await update($, notice, () => 'No se pudo guardar el equipo en este ordenador.')
      }
    }

    if (mode !== 'closed') {
      const saved = await read($, peers)
      const problem = await read($, notice)
      const close = <Button key="close" label="✕" onPress={() => update($, picker, () => 'closed' as VisorPicker)} />

      if (mode === 'list') {
        return (
          <Box flexDirection="column">
            <Box flexDirection="row" justifyContent="space-between">
              <Text bold>¿A qué ordenador me conecto?</Text>
              {close}
            </Box>
            <Box flexDirection="row" flexWrap="wrap" columnGap={1}>
              {saved.map(p => (
                <Button
                  key={`peer-${p.name}`}
                  label={p.name === p.id ? spaced(p.id) : `${p.name} · ${spaced(p.id)}`}
                  onPress={() => connect(p.name)}
                />
              ))}
              <Button key="add" label="+ Agregar nuevo" onPress={() => update($, picker, () => 'new' as VisorPicker)} />
            </Box>
            <Text dimColor>La contraseña de estos equipos ya está guardada en este ordenador.</Text>
          </Box>
        )
      }

      return (
        <Box flexDirection="column">
          <Box flexDirection="row" justifyContent="space-between">
            <Text bold>Conectar a un ordenador nuevo</Text>
            {close}
          </Box>
          <Box flexDirection="row" columnGap={2}>
            <Box flexDirection="column" width="50%">
              <Input
                key="id"
                label="ID"
                placeholder="123 456 789"
                submitLabel="guardar y conectar"
                autoFocus
                onInput={value => {
                  draftId = value
                }}
                onSubmit={value => {
                  draftId = value
                  void save()
                }}
              />
            </Box>
            <Box flexDirection="column" width="50%">
              <Input
                key="password"
                label="Contraseña"
                placeholder="la contraseña permanente"
                submitLabel="guardar y conectar"
                onInput={value => {
                  draftPassword = value
                }}
                onSubmit={value => {
                  draftPassword = value
                  void save()
                }}
              />
            </Box>
          </Box>
          {problem !== '' ? <Text color="warning">{problem}</Text> : null}
          <Box flexDirection="row" columnGap={1}>
            <Button key="save" label="Guardar y conectar" onPress={() => save()} />
            {saved.length > 0 ? (
              <Button key="back" label="Ver guardados" onPress={() => update($, picker, () => 'list' as VisorPicker)} />
            ) : null}
          </Box>
          <Text dimColor>Se guarda solo en este ordenador. Claude no ve la contraseña.</Text>
        </Box>
      )
    }

    if (e.surface !== 'desktop' || frame === '') {
      return next(e)
    }

    const { Svg } = $.ui.resolve(e)

    if (await read($, isHidden)) {
      return (
        <Box>
          <Button key="show" label="Mostrar visor remoto" onPress={() => update($, isHidden, () => false)} />
        </Box>
      )
    }

    const steps = await read($, plan)
    const doing = await read($, activity)
    const current = Math.max(0, steps.findIndex(s => s.status === 'in_progress'))
    const first = Math.max(0, Math.min(current - 2, steps.length - MAX_ROWS))
    const shown = steps.slice(first, first + MAX_ROWS)
    const left = steps.filter(s => s.status !== 'completed').length

    return (
      <Box flexDirection="row">
        <Svg source={frame} alt="Pantalla del ordenador remoto" width={427} height={240} />
        <Box flexDirection="column" flexGrow={1} marginLeft={2}>
          <Box flexDirection="row" justifyContent="space-between">
            <Text bold>{steps.length > 0 ? `Tareas: quedan ${left} de ${steps.length}` : 'Ordenador remoto'}</Text>
            <Button key="hide" label="✕" onPress={() => update($, isHidden, () => true)} />
          </Box>
          {doing !== '' ? <Text color="warning">Ahora: {doing}</Text> : null}
          {steps.length === 0 ? <Text dimColor>Sin tareas en curso</Text> : null}
          {shown.map((s, i) =>
            s.status === 'completed' ? (
              <Text key={`s${first + i}`} dimColor>
                ✓ {s.content}
              </Text>
            ) : s.status === 'in_progress' ? (
              <Text key={`s${first + i}`} bold>
                ▸ {s.content}
              </Text>
            ) : (
              <Text key={`s${first + i}`}>○ {s.content}</Text>
            ),
          )}
          {first + MAX_ROWS < steps.length ? <Text dimColor>… y {steps.length - first - MAX_ROWS} más</Text> : null}
        </Box>
      </Box>
    )
  })
}

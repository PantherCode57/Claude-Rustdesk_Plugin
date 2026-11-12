import { atom, read, update } from 'claude-code'
import type { Register } from 'claude-code'

import type { VisorStep } from '../types'

// The remote screen as an SVG (a small JPEG inside it, Claude's pointer drawn over it), served
// by the bridge. Empty while there is no remote session: then the band is not drawn at all.
const FRAME_URL = 'http://127.0.0.1:21500/frame.svg?max=640'
const MAX_ROWS = 8

const svg = atom({ plugin: 'rustdesk-mcp', key: 'svg' } as const, '')
const isHidden = atom({ plugin: 'rustdesk-mcp', key: 'isHidden' } as const, false)
const plan = atom({ plugin: 'rustdesk-mcp', key: 'plan' } as const, [] as VisorStep[])
const activity = atom({ plugin: 'rustdesk-mcp', key: 'activity' } as const, '')

type Args = Record<string, unknown>

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

  on('session.start', async ($, e, next) => {
    $.clock.every(1000, async () => {
      let frame = ''
      try {
        const r = await $.http.fetch(FRAME_URL)
        frame = r.ok ? r.text : ''
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

  on('tool.call', async ($, e, next) => {
    const tool: string = e.tool
    const a = e as unknown as Args
    const remote = /rustdesk_([a-z_]+)$/.exec(tool)

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

    return next(e)
  }).catch(($, e, next) => next(e))

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const frame = await read($, svg)

    if (e.surface !== 'desktop' || frame === '') {
      return next(e)
    }

    const { Box, Button, Svg, Text } = $.ui.resolve(e)

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

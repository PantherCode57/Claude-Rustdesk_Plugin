export type VisorFrame = string
export type VisorStep = { content: string; status: string }
export type VisorPeer = { name: string; id: string }
export type VisorPicker = 'closed' | 'list' | 'new'

declare module 'claude-code' {
  interface PluginState {
    'rustdesk-mcp': {
      svg: VisorFrame
      isHidden: boolean
      plan: VisorStep[]
      activity: string
      picker: VisorPicker
      peers: VisorPeer[]
      notice: string
      port: number
    }
  }
}

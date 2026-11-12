export type VisorFrame = string
export type VisorStep = { content: string; status: string }

declare module 'claude-code' {
  interface PluginState {
    'rustdesk-mcp': {
      svg: VisorFrame
      isHidden: boolean
      plan: VisorStep[]
      activity: string
    }
  }
}

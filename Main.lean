import FormalWeb
import FormalWeb.FFI


@[export formal_web_handle_winit_redraw]
def handleWinitRedraw (message : String) : IO Unit := do
  FormalWeb.sendRuntimeMessage s!"Lean redraw callback: {message}"

def main : IO Unit := do
  FormalWeb.runWinitEventLoop ()

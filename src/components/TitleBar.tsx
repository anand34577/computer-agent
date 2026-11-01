import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Bot, Copy, Minus, PanelLeft, Search, Square, X } from "lucide-react";

/** The app's own title bar: drag area, command search and window controls. */
export default function TitleBar({ name, onSearch, onToggleSidebar }: { name: string; onSearch: () => void; onToggleSidebar: () => void }) {
  const win = getCurrentWindow();
  const [max, setMax] = useState(false);
  useEffect(() => {
    win.isMaximized().then(setMax).catch(() => {});
    const un = win.onResized(() => win.isMaximized().then(setMax).catch(() => {}));
    return () => {
      un.then((f) => f());
    };
  }, []);
  return (
    <header className="titlebar" data-tauri-drag-region onDoubleClick={(e) => e.target === e.currentTarget && win.toggleMaximize()}>
      <div className="tb-left" data-tauri-drag-region>
        <button className="icon-btn" onClick={onToggleSidebar} aria-label="Toggle sidebar" title="Toggle sidebar (Ctrl+B)"><PanelLeft size={16} /></button>
        <div className="brand-mark" style={{ width: 22, height: 22, borderRadius: 6 }} data-tauri-drag-region><Bot size={13} /></div>
        <span className="tb-name ellipsis" data-tauri-drag-region>{name}</span>
      </div>
      <div className="tb-center" data-tauri-drag-region>
        <button className="tb-search" onClick={onSearch} aria-label="Search and commands">
          <Search size={14} /> Search chats, commands and settings
          <span className="kbd">Ctrl K</span>
        </button>
      </div>
      <div className="tb-controls">
        <button onClick={() => win.minimize()} aria-label="Minimize"><Minus size={16} /></button>
        <button onClick={() => win.toggleMaximize()} aria-label={max ? "Restore" : "Maximize"}>{max ? <Copy size={13} /> : <Square size={13} />}</button>
        <button className="close" onClick={() => win.close()} aria-label="Close"><X size={17} /></button>
      </div>
    </header>
  );
}

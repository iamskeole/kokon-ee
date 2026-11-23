// The top bar: the project's title, then undo/redo, the update pill, the status capsule,
// private mode, the Help menu (shortcuts, video guides, prompt tips) and (only while the sidebar is hidden) Settings, then the window's
// buttons. Its own file so the stage's boot sequence can share it.
import { CircleHelp, CirclePlay, EyeOff, Keyboard, Lightbulb, PanelLeft, Redo2, Settings, SquarePen, Undo2 } from "lucide-react";
import type { ReactNode } from "react";
import { native } from "../lib/native";
import { haptic } from "../lib/haptics";
import { isNarrow, shortcut } from "../lib/platform";
import { getState, newProject, redoVersion, setPrivateMode, setState, toastError, undoKey, undoVersion, useStore } from "../lib/store";
import { SilkSpin } from "./Cocoon";
import { ProjectTitle, Wordmark } from "./Sidebar";
import { VIDEOS } from "../lib/videos";
import { IconButton, Popover } from "./ui";

/** `quiet`: nothing while the model is loaded and idle (the phone bar has no room for "all is well"). */
export function StatusIndicator({ quiet = false }: { quiet?: boolean }) {
  const connected = useStore((s) => s.connected);
  const server = useStore((s) => s.server);
  const external = useStore((s) => s.external);
  const busy = useStore((s) => Object.values(s.jobs).some((j) => !["done", "failed", "cancelled"].includes(j.status)));
  let cls = "dot";
  let text = "Connecting";
  let title = "";
  if (connected) {
    switch (server.state) {
      case "ready":
        cls = "dot ready";
        text = busy ? "Working" : "Ready";
        title = server.owned ? "Model loaded" : "Loaded on demand: " + (server.model ?? "the model");
        break;
      case "starting":
        cls = "dot busy";
        text = server.model ? "Loading " + server.model : "Loading model";
        break;
      case "error":
        cls = "dot error";
        text = server.error?.message ?? "Inference server error";
        title = "Open Diagnostics";
        break;
      default:
        text = external ? "No model loaded" : "Model not loaded";
        // An externally managed engine loads on its own schedule (it loads when a job runs):
        // this is a display, not a request. A click that asks the endpoint to load can evict
        // whatever it is serving, so only Settings → Performance makes that explicit request.
        title = external ? "The model loads when you generate an image" : "Click to load the model now";
    }
  }
  if (quiet && cls === "dot ready" && !busy) return null;
  return (
    <button
      className="status"
      title={title}
      onClick={() => {
        if (server.state === "error") setState({ overlay: "settings" });
      }}
    >
      {cls === "dot busy" || (cls === "dot ready" && busy) ? <SilkSpin size={18} /> : <span className={cls} />}
      <span style={{ maxWidth: 360, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{text}</span>
    </button>
  );
}

/** Undo / Redo between versions: back to the image the current one was made from. */
function HistoryButtons() {
  const canUndo = useStore((s) => undoKey(s) !== null);
  const canRedo = useStore((s) => s.redo.length > 0);
  if (!canUndo && !canRedo) return null;
  return (
    <>
      <IconButton title={`Undo: back to the previous version (${shortcut("Z")})`} onClick={undoVersion} disabled={!canUndo}>
        <Undo2 size={18} />
      </IconButton>
      <IconButton title={`Redo (${shortcut("Y")})`} onClick={redoVersion} disabled={!canRedo}>
        <Redo2 size={18} />
      </IconButton>
    </>
  );
}

function PrivateToggle() {
  const on = useStore((s) => s.privateMode);
  return on ? (
    <button className="private-pill" onClick={togglePrivateMode} title="Private mode is on: images are not saved and are deleted when you turn it off or close the app. Click to turn it off.">
      <EyeOff size={15} /> Private
    </button>
  ) : (
    <IconButton title="Private mode: nothing is saved to the gallery or your Pictures folder" onClick={togglePrivateMode}>
      <EyeOff size={18} />
    </IconButton>
  );
}

/** Private mode from the phone's library: turning it off asks first (its images are deleted). */
export function togglePrivateMode() {
  if (isNarrow()) haptic();
  setPrivateMode(!getState().privateMode, () => native.confirm("Turn off private mode? The images made in private mode will be deleted.", "Private mode"));
}

/** The "?" button: shortcuts, the video guides on YouTube (opened in the browser) and the prompt tips. */
export function HelpMenu() {
  const item = (icon: ReactNode, label: string, onClick: () => void, sub?: string) => (
    <button className="opt" role="menuitem" onClick={onClick}>
      <span className="check">{icon}</span>
      <span>{label}</span>
      {sub && <span className="sub">{sub}</span>}
    </button>
  );
  const video = (v: { url: string; title: string }, close: () => void) =>
    item(<CirclePlay size={15} />, v.title, () => (close(), native.openLink(v.url).catch(toastError)));
  return (
    <Popover
      className="help-menu"
      placement="bottom-end"
      trigger={(open, toggle) => (
        <button className={`icon-btn ${open ? "active" : ""}`} title="Help" aria-label="Help" aria-haspopup="menu" aria-expanded={open} onClick={toggle}>
          <CircleHelp size={18} />
        </button>
      )}
    >
      {(close) => (
        <div role="menu" aria-label="Help">
          {item(<Keyboard size={15} />, "Keyboard shortcuts", () => (close(), setState({ overlay: "shortcuts" })), "?")}
          {item(<Lightbulb size={15} />, "Prompt tips", () => (close(), setState({ overlay: "prompt-tips" })))}
          <div className="menu-sep" />
          <div className="label">Video guides · YouTube</div>
          {video(VIDEOS.useCases, close)}
          {video(VIDEOS.update012, close)}
          {video(VIDEOS.remote011, close)}
        </div>
      )}
    </Popover>
  );
}
export function TopBar() {
  const overlay = useStore((s) => s.overlay);
  const sidebar = useStore((s) => s.sidebar);
  return (
    <div className="topbar">
      <div className="brand">
        {!sidebar && <Wordmark />}
        {!sidebar && (
          <>
            <IconButton title="Show sidebar" onClick={() => setState({ sidebar: true })}>
              <PanelLeft size={18} />
            </IconButton>
            <IconButton title="New image" onClick={newProject}>
              <SquarePen size={18} />
            </IconButton>
          </>
        )}
        {overlay === "gallery" ? <span className="project-title">All images</span> : <ProjectTitle />}
      </div>
      <div className="toolbar">
        <HistoryButtons />
        <StatusIndicator />
        <PrivateToggle />
        <HelpMenu />
        {/* Settings lives at the foot of the sidebar; here only while the sidebar is hidden. */}
        {!sidebar && (
          <IconButton title="Settings" onClick={() => setState({ overlay: "settings" })}>
            <Settings size={18} />
          </IconButton>
        )}
      </div>
    </div>
  );
}

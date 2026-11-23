import { CirclePlay, Images, Layers } from "lucide-react";
import { api } from "../lib/api";
import { whatsNewDue as due } from "../lib/logic";
import { refreshStatus, setState, toastError } from "../lib/store";
import { native } from "../lib/native";
import { VIDEOS } from "../lib/videos";
import { KokonMark } from "./Welcome";

/** The release these notes describe. Shown once to people updating from an earlier version. */
export const WHATS_NEW_VERSION = "0.12.1";

const NEWS = [
  {
    icon: Images,
    title: "More like this",
    text: "Under a new image, More like this makes another one from it: Subtle keeps the picture and changes the details, Strong keeps the idea and changes more. Regenerate still starts from scratch.",
  },
  {
    icon: Layers,
    title: "Redraw at this size",
    text: "In the Layers lab, an object made bigger with the corner handle can be drawn again at its new size, so it stays sharp. The stretched version stays under Versions.",
    video: VIDEOS.update012.url,
  },
];

/** Whether to show the notes at start (the pure function lives in logic). */
export function whatsNewDue(ui: Record<string, unknown>): boolean {
  return due(ui, WHATS_NEW_VERSION);
}

/** What's new in this release (pure notes: the downloads the old version offered are gone —
 * the box's models are pinned by the owner, not offered here). */
export function WhatsNew() {
  const done = async () => {
    try {
      const cfg = await api.config();
      const ui: Record<string, unknown> = { ...((cfg.ui ?? {}) as Record<string, unknown>), whatsNewSeen: WHATS_NEW_VERSION };
      await api.saveConfig({ ...cfg, ui });
    } catch (e) {
      toastError(e);
    }
    setState({ overlay: null });
    refreshStatus();
  };

  return (
    <div className="modal-scrim">
      <div className="welcome" role="dialog" aria-label={`What's new in Kokon ${WHATS_NEW_VERSION}`}>
        <KokonMark size={44} />
        <h1>What's new in Kokon {WHATS_NEW_VERSION}</h1>
        <div className="welcome-grid">
          {NEWS.map(({ icon: Icon, title, text, video }: { icon: typeof Layers; title: string; text: string; video?: string }) => (
            <div className="welcome-item" key={title}>
              <Icon size={20} />
              <strong>{title}</strong>
              <span>
                {text}
                {video && (
                  <>
                    {" "}
                    <button className="link-btn" onClick={() => native.openLink(video).catch(toastError)} title="On YouTube (opens in your browser)">
                      <CirclePlay size={13} /> Watch the 0.12 video
                    </button>
                  </>
                )}
              </span>
            </div>
          ))}
        </div>
        <button className="btn primary big" onClick={done}>
          Got it
        </button>
      </div>
    </div>
  );
}

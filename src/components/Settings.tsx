import { Activity, Copy, Cpu, HardDrive, Heart, Layers, RefreshCw, RotateCcw, Settings2, Users, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { api } from "../lib/api";
import { AiMarkingCard } from "./AiCheck";
import { People } from "./People";
import { native } from "../lib/native";
import { getState, refreshStatus, setState, startTour, toastError, toastInfo, useStore } from "../lib/store";
import { setThemeChoice, themeChoice, type ThemeChoice } from "../lib/theme";
import { LoraSettings } from "./LoraSettings";
import { UpscalerControls } from "./Upscalers";
import { WHATS_NEW_VERSION } from "./WhatsNew";
import type { AppConfig, TextEncoderPlacement } from "../lib/types";
import { IconButton, Seg, Switch } from "./ui";

const REPO_URL = "https://github.com/GabrieleLozupone/kokon";
const SUPPORT_URL = "https://kokon.app/support/";

/** Top of Settings → General: where Kokon lives. */
export function RepoCard() {
  return (
    <div className="repo-card">
      <GitHubMark />
      <div className="body">
        <strong>Kokon is open source</strong>
        <span className="muted">Code, issues and discussions are on GitHub. A star helps other people find it.</span>
      </div>
      <button className="btn primary" onClick={() => native.openLink(REPO_URL).catch(toastError)}>
        Open GitHub
      </button>
    </div>
  );
}

/** Under the repository card: one quiet way to tip, no prompts anywhere else. */
export function SupportCard() {
  return (
    <div className="repo-card support-card">
      <Heart size={22} className="support-mark" aria-hidden />
      <div className="body">
        <strong>Support Kokon</strong>
        <span className="muted">Kokon is free and made by one person. If it's useful to you, a tip helps keep it that way.</span>
      </div>
      <button className="btn" onClick={() => native.openLink(SUPPORT_URL).catch(toastError)}>
        Support
      </button>
    </div>
  );
}

/** GitHub's mark (Octicons mark-github, MIT); lucide no longer ships brand icons. */
function GitHubMark() {
  return (
    <svg className="github-mark" viewBox="0 0 16 16" width="26" height="26" aria-hidden>
      <path
        fill="currentColor"
        d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"
      />
    </svg>
  );
}

type Tab = "general" | "models" | "lora" | "performance" | "people" | "diagnostics";
const TABS: Tab[] = ["general", "models", "lora", "performance", "people", "diagnostics"];

/** Browser: what the computer running Kokon has set (only its desktop app changes folders and files). */
/** The service box's folders: read-only in the browser — the box's own settings file owns them. */
function PathField(props: { label: string; value: string }) {
  return (
    <div className="field">
      <span>{props.label}</span>
      <span className="muted">{props.value || "—"} (on the computer running Kokon)</span>
    </div>
  );
}

/** One of the prompt enhancer's instructions: the user's text, or the built-in one ("" saved). */
function EnhancerPrompt(props: { label: string; help: string; value: string; builtin: string | null; max: number; onChange: (v: string) => void }) {
  const custom = props.value.trim() !== "" && props.value.trim() !== props.builtin?.trim();
  return (
    <div className="field">
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ flex: 1 }}>
          {props.label} <span className="muted">· {custom ? "your instructions" : "built-in"}</span>
        </span>
        <button className="btn" disabled={!custom} onClick={() => props.onChange("")} title="Go back to Kokon's built-in instructions">
          <RotateCcw size={13} /> Restore default
        </button>
      </div>
      <textarea
        className="input"
        rows={9}
        spellCheck={false}
        maxLength={props.max}
        value={props.value || props.builtin || ""}
        placeholder={props.builtin === null ? "Loading…" : ""}
        onChange={(e) => props.onChange(e.target.value.trim() === props.builtin?.trim() ? "" : e.target.value)}
      />
      <span className="muted">{props.help}</span>
    </div>
  );
}

function Diagnostics() {
  const [text, setText] = useState<string | null>(null);
  const load = async () => {
    setText(null);
    try {
      setText((await api.diagnostics()).text);
    } catch (e) {
      toastError(e);
      setText("");
    }
  };
  useEffect(() => {
    load();
  }, []);
  return (
    <>
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        {window.isSecureContext && (
          <button className="btn" disabled={!text} onClick={async () => (await native.copyText(text ?? ""), toastInfo("Diagnostics copied"))}>
            <Copy size={14} /> Copy
          </button>
        )}
        <button className="btn" onClick={load}>
          <RefreshCw size={14} /> Refresh
        </button>
      </div>
      <pre className="diag">{text ?? "Collecting…"}</pre>
    </>
  );
}

export function Settings() {
  // Opened on a tab by `openSettings` (e.g. the LoRA badge), else General.
  const [tab, setTab] = useState<Tab>(() => {
    const t = getState().settingsTab as Tab | null;
    return t && TABS.includes(t) ? t : "general";
  });
  useEffect(() => setState({ settingsTab: null }), []);
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  // Opened on a section (e.g. Prompt+ instructions, from the Prompt+ menu): scrolled to once it is drawn.
  const focus = useRef(getState().settingsFocus);
  useEffect(() => {
    if (!focus.current || !cfg) return;
    const el = document.getElementById(`settings-${focus.current}`);
    if (!el) return;
    focus.current = null;
    setState({ settingsFocus: null });
    window.requestAnimationFrame(() => el.scrollIntoView({ block: "start", behavior: "smooth" }));
  }, [cfg, tab]);
  // Who is looking: the named account's name (null = the operator or the desktop), and
  // whether a remote password is set. Both come from the session endpoint, which the
  // desktop calls with its token (no cookie, so `user` is null there).
  const [who, setWho] = useState<{ user: string | null; password_set: boolean } | null>(null);
  const up = useStore((st) => st.status?.upscaler);
  // The saved model setup: what is missing, whether the official files are downloaded.
  const [dirty, setDirty] = useState(false);
  const [theme, setTheme] = useState(themeChoice);
  const [enhancerDefaults, setEnhancerDefaults] = useState<{ t2i: string; edit: string; compose?: string; max_chars: number } | null>(null);
  const textGeneration = useStore((s) => s.status?.server.text_generation);

  useEffect(() => {
    api.config().then(setCfg, toastError);
    api.session().then(setWho, () => {});
    api.enhancerDefaults().then(setEnhancerDefaults, () => {});
  }, []);

  // Leaving with unsaved changes asks first (people forgot Save): `then` runs after Save or
  // Discard, Cancel stays here.
  const [leaving, setLeaving] = useState<(() => void) | null>(null);
  const leave = (then: () => void) => (dirty ? setLeaving(() => then) : then());
  const close = () => leave(() => setState({ overlay: null }));
  const onEscape = useRef(close);
  onEscape.current = () => (leaving ? setLeaving(null) : close());
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onEscape.current();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!cfg) return null;
  const profile = cfg.profiles.find((p) => p.id === cfg.activeProfile) ?? cfg.profiles[0];
  // Apple Silicon (Metal) has unified memory: no VRAM, device picker or CUDA-only options.
  const mac = cfg.gpuBackend === "metal";
  const appVersion = getState().status?.service.version ?? "";
  const set = (patch: Partial<AppConfig>) => {
    setCfg({ ...cfg, ...patch });
    setDirty(true);
  };
  // Upscaler options apply at once, like in the Advanced menu: saved on top of the stored
  // settings (other unsaved fields stay a draft) and mirrored in the draft, so Save keeps them.
  const applyNow = async (patch: Partial<AppConfig>) => {
    setCfg((c) => (c ? { ...c, ...patch } : c));
    try {
      const saved = await api.config();
      await api.saveConfig({ ...saved, ...patch });
      refreshStatus();
    } catch (e) {
      toastError(e);
    }
  };

  const save = async (): Promise<boolean> => {
    try {
      const r = await api.saveConfig(cfg);
      setDirty(false);
      toastInfo(r.backend_restart_needed ? "Saved. The model reloads with the new settings on the next generation." : "Saved");
      refreshStatus();
      return true;
    } catch (e) {
      toastError(e);
      return false;
    }
  };

  const nav: [Tab, string, React.ReactNode][] = [
    ["general", "General", <Settings2 size={16} key="g" />],
    ["models", "Models", <HardDrive size={16} key="m" />],
    ["lora", "LoRA (beta)", <Layers size={16} key="l" />],
    ["performance", "Performance", <Cpu size={16} key="p" />],
    // Named accounts: shown where a password is set and the viewer is not one of the
    // accounts themselves (D-MU11); named accounts get today's settings, without it.
    ...(who && !who.user && who.password_set ? ([["people", "People", <Users size={16} key="pe" />]] as [Tab, string, React.ReactNode][]) : []),
    ["diagnostics", "Diagnostics", <Activity size={16} key="d" />],
  ];

  return (
    <div className="modal-scrim" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal" role="dialog" aria-label="Settings">
        <div className="modal-nav">
          {nav.map(([t, label, icon]) => (
            <button key={t} className={tab === t ? "on" : ""} onClick={() => setTab(t)}>
              {icon} {label}
            </button>
          ))}
          <span style={{ flex: 1 }} />
          {tab !== "diagnostics" && tab !== "lora" && tab !== "people" && (
            <button className="btn primary nav-save" style={{ justifyContent: "center" }} disabled={!dirty} onClick={save}>
              Save
            </button>
          )}
        </div>
        <div className="modal-body">
          <div style={{ display: "flex", alignItems: "center" }}>
            <h3 style={{ flex: 1 }}>{nav.find((n) => n[0] === tab)?.[1]}</h3>
            <IconButton title="Close" onClick={close}>
              <X size={18} />
            </IconButton>
          </div>

          {tab === "general" && (
            <>
              <RepoCard />
              <SupportCard />
              <AiMarkingCard />
              <PathField label="Save images to" value={cfg.outputDir} />
              <PathField label="stable-diffusion.cpp folder" value={cfg.sdcppDir} />
              <div className="field-row">
                <span>A folder for each project</span>
                <Switch on={!!cfg.projectFolders} onChange={(v) => set({ projectFolders: v })} />
              </div>
              <span className="muted" style={{ marginTop: -10 }}>New images go to a folder named after their project, with the date in the file name, instead of one folder per day. Images already saved stay where they are.</span>
              <div className="field">
                <span>Appearance</span>
                <Seg<ThemeChoice>
                  value={theme}
                  options={[
                    { value: "system", label: "System" },
                    { value: "light", label: "Light" },
                    { value: "dark", label: "Dark" },
                  ]}
                  onChange={(v) => (setThemeChoice(v), setTheme(v))}
                />
                <span className="muted">System follows your computer's light or dark mode. Applies at once.</span>
              </div>
              <div className="field-row">
                <span>Kokon {appVersion} on the computer running it.</span>
                <button className="btn" onClick={() => api.logout().then(() => location.reload(), toastError)}>
                  Sign out
                </button>
              </div>
              <div className="field-row">
                <span>What's new in Kokon {WHATS_NEW_VERSION}</span>
                <button className="btn" onClick={() => leave(() => setState({ overlay: "whats-new" }))}>
                  Show
                </button>
              </div>
              <div className="field-row">
                <span>Welcome screen: what Kokon is and how it keeps your work private</span>
                <button className="btn" onClick={() => leave(() => setState({ overlay: "welcome" }))}>
                  Show
                </button>
              </div>
              <div className="field-row">
                <span>Tour: a short walk through an example project (adds the examples again if you removed them)</span>
                <button className="btn" onClick={() => leave(startTour)}>
                  Start
                </button>
              </div>

              <h4 id="settings-prompt-plus" style={{ margin: "14px 0 0", fontSize: 14, fontWeight: 600, scrollMarginTop: 12 }}>Prompt+</h4>
              <span className="muted" style={{ marginTop: -8 }}>
                With Prompt+ on (next to the prompt), the model's own language model rewrites your prompt before making the image, following these instructions. It
                always writes in English. Its menu also lets you read and change the prompt before the image is made.
                {textGeneration === false && " The inference engine in use cannot write prompts: Prompt+ appears with an engine that can."}
              </span>
              <div className="setting-card" style={{ alignItems: "flex-start" }}>
                <span className="muted">
                  Kokon was tested only with its built-in instructions. Your own can make results worse, or make the model answer with something that is not a prompt; Restore
                  default brings the built-in ones back.
                </span>
              </div>
              <EnhancerPrompt
                label="New images"
                help="Used when creating an image from the prompt alone."
                value={cfg.enhancerT2iPrompt ?? ""}
                builtin={enhancerDefaults?.t2i ?? null}
                max={enhancerDefaults?.max_chars ?? 20000}
                onChange={(v) => set({ enhancerT2iPrompt: v })}
              />
              <EnhancerPrompt
                label="Edits"
                help="Used when editing or combining images; the model sees the images."
                value={cfg.enhancerEditPrompt ?? ""}
                builtin={enhancerDefaults?.edit ?? null}
                max={enhancerDefaults?.max_chars ?? 20000}
                onChange={(v) => set({ enhancerEditPrompt: v })}
              />
              <EnhancerPrompt
                label="New images from references"
                help="Used for a new image that uses your images as references (New image with images added). Each image is first labelled on its own; these instructions then write the scene from the labels."
                value={cfg.enhancerComposePrompt ?? ""}
                builtin={enhancerDefaults?.compose ?? null}
                max={enhancerDefaults?.max_chars ?? 20000}
                onChange={(v) => set({ enhancerComposePrompt: v })}
              />
            </>
          )}

          {tab === "people" && <People />}

          {tab === "models" && (
            <>
              {cfg.externalEngine ? (
                <div className="setting-card">
                  <div className="setting-card-text">
                    <strong>External engine</strong>
                    <span className="muted">
                      {cfg.externalEngine.modelName} is managed outside Kokon by a gateway ({cfg.externalEngine.gateway}); Kokon never
                      starts, stops or downloads it, and "no model loaded" is its resting state.
                      {(cfg.models?.length ?? 0) > 0 ? " Pick another model with the model picker in the prompt box." : ""}
                    </span>
                  </div>
                </div>
              ) : (
                <div className="setting-card">
                  <div className="setting-card-text">
                    <strong>Model</strong>
                    <span className="muted">
                      {profile.displayName}. On this box the model files live in the stable-diffusion.cpp folder; a box that runs its
                      engine elsewhere keeps them on that box (the fresh-box doc names the folder).
                    </span>
                  </div>
                  {cfg.profiles.length > 1 ? (
                    <select className="select" value={profile.id} onChange={(e) => set({ activeProfile: e.target.value })}>
                      {cfg.profiles.map((pp) => (
                        <option key={pp.id} value={pp.id}>
                          {pp.displayName}
                        </option>
                      ))}
                    </select>
                  ) : null}
                </div>
              )}
              <PathField label="stable-diffusion.cpp folder" value={cfg.sdcppDir} />
              <UpscalerControls
                model={cfg.upscalerModel}
                blend={cfg.upscaleBlend}
                onModel={(m) => applyNow({ upscalerModel: m })}
                onBlend={(b) => setCfg({ ...cfg, upscaleBlend: b })}
                onBlendDone={() => applyNow({ upscaleBlend: cfg.upscaleBlend })}
              />
              {up && up.available === false && (
                <span className="muted">The upscaler model is not on this box; enlargements use Lanczos until it is pinned (the fresh-box doc names the folder).</span>
              )}
              <div className="field-row">
                <span>Shrink the image before upscaling</span>
                <Switch on={cfg.upscalePrescale} onChange={(v) => applyNow({ upscalePrescale: v })} />
              </div>
              <span className="muted" style={{ marginTop: -10 }}>
                Gives the upscaler a smaller image, like the ones it was trained on: fewer invented patterns on clean renders and faster, slightly less fine detail. Only for enlargements under 2.8×.
              </span>
            </>
          )}


          {tab === "performance" && mac && (
            <>
              <div className="field">
                <span>Model memory</span>
                <Seg<TextEncoderPlacement>
                  value={cfg.textEncoderPlacement === "disk" ? "disk" : "gpu"}
                  options={[
                    { value: "gpu", label: "Keep loaded" },
                    { value: "disk", label: "Load when needed" },
                  ]}
                  onChange={(v) => set({ textEncoderPlacement: v })}
                />
                <span className="muted">
                  Keep loaded: every model stays in memory, fastest between steps; needs 24 GB or more. Load when needed: each model (text encoder, image model, decoder) is loaded from the SSD when it runs and released after, so the image model has room to work on 16 GB Macs. It adds a few seconds per image. macOS cannot swap memory the GPU is using, so this is what makes 16 GB work.
                </span>
              </div>
              <label className="field-row">
                <span>Memory kept free for macOS (GiB)</span>
                <input className="input" type="number" min={0} max={16} step={0.5} style={{ width: 90 }} value={cfg.vramReserveGib} onChange={(e) => set({ vramReserveGib: Math.max(0, Number(e.target.value) || 0) })} />
              </label>
            </>
          )}

          {tab === "performance" && !mac && (
            <>
              <div className="field">
                <span>Engine</span>
                <span>{cfg.gpuBackend === "vulkan" ? "Vulkan" : "CUDA (NVIDIA)"}</span>
                <span className="muted">A pinned stock build (the fresh-box doc names it); the engine the box runs is reported in Diagnostics.</span>
              </div>
              <div className="field">
                <span>Text encoder weights</span>
                <Seg<TextEncoderPlacement>
                  value={cfg.textEncoderPlacement}
                  options={[
                    { value: "ram", label: "RAM" },
                    { value: "gpu", label: "GPU" },
                    { value: "cpu", label: "CPU" },
                    { value: "auto", label: "Auto" },
                  ]}
                  onChange={(v) => set({ textEncoderPlacement: v })}
                />
                <span className="muted">
                  RAM (recommended): weights in system memory, computed on the GPU. CPU uses the least VRAM but makes image editing several times slower.
                </span>
              </div>
              <label className="field-row">
                <span>VRAM reserve (GiB)</span>
                <input className="input" type="number" min={0} max={16} step={0.5} style={{ width: 90 }} value={cfg.vramReserveGib} onChange={(e) => set({ vramReserveGib: Math.max(0, Number(e.target.value) || 0) })} />
              </label>
            </>
          )}

          {tab === "performance" && (
            <>
              <div className="field-row">
                <span>Flash attention</span>
                <Switch on={cfg.flashAttention} onChange={(v) => set({ flashAttention: v })} />
              </div>
              {cfg.gpuBackend === "cuda" && (
                <>
                  <div className="field-row">
                    <span>SageAttention (faster sampling)</span>
                    <Switch on={cfg.sageAttention && !cfg.sageUnsupported} disabled={cfg.sageUnsupported} onChange={(v) => set({ sageAttention: v })} />
                  </div>
                  {cfg.sageUnsupported && <span className="muted" style={{ marginTop: -10 }}>Needs an RTX 30 series or newer card; off on this one.</span>}
                </>
              )}
              <label className="field-row">
                <span>VAE tile size (px, 0 = off)</span>
                <input className="input" type="number" min={0} max={2048} step={128} style={{ width: 90 }} value={cfg.vaeTileSize} onChange={(e) => set({ vaeTileSize: Math.max(0, Math.round(Number(e.target.value) || 0)) })} />
              </label>
              {cfg.gpuBackend === "vulkan" && <span className="muted" style={{ marginTop: -10 }}>With Vulkan, images are decoded in tiles of 512 px at most: larger tiles exceed the driver's buffer limit on AMD cards and get much slower.</span>}
              <div className="field">
                <span>Edit prefix cache</span>
                <Seg<string>
                  value={cfg.prefixCacheType}
                  options={[
                    { value: "q8_0", label: "8-bit" },
                    { value: "f16", label: "FP16" },
                    { value: "auto", label: "Auto" },
                    { value: "off", label: "Off" },
                  ]}
                  onChange={(v) => set({ prefixCacheType: v })}
                />
                <span className="muted">
                  Reuses the reference image and prompt across steps so each step only computes the new image. 8-bit (recommended) uses the least memory; FP16 is more exact but needs twice the memory{cfg.gpuBackend === "cuda" ? "; Auto uses FP32 when SageAttention is on and usually does not fit" : ""}.
                </span>
              </div>
              {cfg.textEncoderPlacement !== "disk" && (
                <div className="field-row">
                  <span>Load all weights at startup</span>
                  <Switch on={cfg.eagerLoad} onChange={(v) => set({ eagerLoad: v })} />
                </div>
              )}
              <label className="field-row">
                <span>Upscaler tile size (px, 0 = automatic)</span>
                <input className="input" type="number" min={0} max={1024} step={64} style={{ width: 90 }} value={cfg.upscaleTileSize} onChange={(e) => set({ upscaleTileSize: Math.max(0, Math.round(Number(e.target.value) || 0)) })} />
              </label>
              <span className="muted" style={{ marginTop: -10 }}>Automatic: 768 px with 16 GB of GPU memory or more (fastest), 256 px below (about 1 GB less).</span>
              {!mac && (
                <>
                  <label className="field-row">
                    <span>Simulated VRAM for testing (MiB, 0 = detect)</span>
                    <input className="input" type="number" min={0} max={196608} step={1024} style={{ width: 100 }} value={cfg.vramOverrideMib} onChange={(e) => set({ vramOverrideMib: Math.max(0, Math.round(Number(e.target.value) || 0)) })} />
                  </label>
                  <span className="muted" style={{ marginTop: -10 }}>
                    Plans image sizes as if the card had this much memory (e.g. 12288 to test a 12 GB card).
                  </span>
                </>
              )}
            </>
          )}

          {tab === "lora" && <LoraSettings />}
          {tab === "diagnostics" && <Diagnostics />}
          {/* On a phone Save sits at the foot of the page (the tabs scroll sideways). */}
          {tab !== "diagnostics" && tab !== "lora" && tab !== "people" && (
            <div className="modal-save">
              <button className="btn primary" disabled={!dirty} onClick={save}>
                Save
              </button>
            </div>
          )}
        </div>
        {leaving && (
          <div className="modal-ask" role="alertdialog" aria-label="Unsaved changes">
            <div className="modal-ask-box">
              <strong>Save your changes?</strong>
              <span className="muted">Some settings were changed and not saved.</span>
              <div className="modal-ask-actions">
                <button className="btn ghost" onClick={() => setLeaving(null)}>
                  Cancel
                </button>
                <button
                  className="btn"
                  onClick={() => {
                    const then = leaving;
                    setLeaving(null);
                    setDirty(false);
                    then();
                  }}
                >
                  Discard
                </button>
                <button
                  className="btn primary"
                  autoFocus
                  onClick={async () => {
                    const then = leaving;
                    setLeaving(null);
                    if (await save()) then();
                  }}
                >
                  Save
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

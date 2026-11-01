import { useEffect, useState } from "react";
import { Bot, CheckCircle2, Hand, Lock, Monitor, Octagon, Plug, Plus, RotateCcw, Scale, ShieldCheck } from "lucide-react";
import { api, errText, type PlatformInfo, type Provider } from "../api";
import { Caps, Card, Callout, useApp } from "../components/common";
import { AddProvider, BrowseModels } from "./ModelsView";
import { PlatformStatus } from "./SettingsView";

const STEPS = ["Welcome", "This computer", "Connect a model", "Safety"];

/** First run: privacy choice → capability check → connect a model → safety basics. */
export default function Onboarding({ onDone }: { onDone: () => void }) {
  const { models, refreshModels, toast, settings } = useApp();
  const [step, setStep] = useState(0);
  const [choice, setChoice] = useState<string | null>(null);
  const [platform, setPlatform] = useState<PlatformInfo | null>(null);
  const [detected, setDetected] = useState<{ name: string; kind: string; base_url: string; running: boolean }[] | null>(null);
  const [adding, setAdding] = useState(false);
  const [browse, setBrowse] = useState<Provider | null>(null);

  useEffect(() => {
    if (step === 1 && !platform) api.platformInfo().then(setPlatform).catch((e) => toast(errText(e), "bad"));
    if (step === 2) { setDetected(null); api.detectLocal().then(setDetected); }
  }, [step]);

  const connect = async (d: { name: string; kind: string; base_url: string }) => {
    try {
      const id = await api.addProvider(d.kind, d.name, d.base_url);
      const p = (await api.listProviders()).find((x) => x.id === id);
      if (p) setBrowse(p);
    } catch (e) {
      toast(errText(e), "bad");
    }
  };

  return (
    <div className="page-inner" style={{ maxWidth: 760, paddingTop: 40 }}>
      <div className="stepper" aria-label="Setup progress">
        {STEPS.map((t, i) => (
          <div key={t} className={i === step ? "on" : i < step ? "done" : ""}>
            <div className="bar" />
            {t}
          </div>
        ))}
      </div>

      {step === 0 && (
        <Card>
          <div className="brand-mark" style={{ width: 44, height: 44, borderRadius: 12, marginBottom: 14 }}><Bot size={24} /></div>
          <h1 style={{ margin: "0 0 6px", fontSize: 24 }}>Welcome to {settings.app_name}</h1>
          <p className="muted" style={{ marginTop: 0 }}>Chat with any AI model and, when it is capable, let it operate your computer safely. You approve plans, risky steps ask first, and one key stops everything.</p>
          <h3 style={{ marginTop: 22 }}>How private should it be?</h3>
          <div className="grid2">
            {[
              { id: "local", icon: Lock, t: "Everything stays on this computer", d: "Local models only. Cloud calls are blocked." },
              { id: "balanced", icon: Scale, t: "Balanced", d: "Use cloud and local models. You always see which provider receives data." },
            ].map((c) => (
              <button key={c.id} className={"choice " + (choice === c.id ? "on" : "")} onClick={() => setChoice(c.id)} aria-pressed={choice === c.id}>
                <div className="choice-icon"><c.icon size={17} /></div>
                <b>{c.t}</b>
                <small>{c.d}</small>
              </button>
            ))}
          </div>
          <div className="row end" style={{ marginTop: 20 }}>
            <button className="btn primary" disabled={!choice} onClick={async () => { await api.applyProfile(choice!); setStep(1); }}>Continue</button>
          </div>
        </Card>
      )}

      {step === 1 && (
        <Card title="What works on this computer" subtitle="Reduced features are labelled, never hidden." icon={Monitor} actions={<button className="btn sm" onClick={() => { setPlatform(null); api.platformInfo().then(setPlatform); }}><RotateCcw size={14} /> Re-check</button>}>
          {platform ? <PlatformStatus p={platform} /> : <div className="muted">Checking…</div>}
          <div className="row between" style={{ marginTop: 20 }}>
            <button className="btn" onClick={() => setStep(0)}>Back</button>
            <button className="btn primary" onClick={() => setStep(2)}>Continue</button>
          </div>
        </Card>
      )}

      {step === 2 && (
        <Card title="Connect a model" subtitle="Use a local runtime you already have, or a cloud provider with your own key. Each model gets a quick self-test." icon={Plug}>
          <div className="stack">
            {detected === null && <div className="muted small">Looking for local runtimes…</div>}
            {detected?.filter((d) => d.running).map((d) => (
              <div className="status-item" key={d.name}>
                <div className="si-icon"><Monitor size={16} /></div>
                <div className="grow"><b>{d.name}</b> <span className="small muted">found on this computer</span></div>
                <button className="btn sm primary" onClick={() => connect(d)}>Connect</button>
              </div>
            ))}
            {detected && detected.every((d) => !d.running) && settings.local_only && (
              <Callout kind="warn">No local runtime found. Install Ollama (ollama.com) or LM Studio, start it, then press Back and Continue to look again. You can also do this later in Models.</Callout>
            )}
            {!settings.local_only && <div><button className="btn" onClick={() => setAdding(true)}><Plus size={15} /> Add a cloud or custom provider</button></div>}
            {models.length > 0 && (
              <div>
                <div className="field-label" style={{ marginBottom: 6 }}><CheckCircle2 size={14} style={{ color: "var(--ok)" }} /> Ready to use</div>
                {models.map((m) => (
                  <div key={m.id} className="row" style={{ padding: "6px 0" }}>
                    <span className="grow">{m.label} <span className="small muted">· {m.provider_name}</span></span>
                    <Caps m={m} />
                  </div>
                ))}
              </div>
            )}
          </div>
          <div className="row between" style={{ marginTop: 20 }}>
            <button className="btn" onClick={() => setStep(1)}>Back</button>
            <button className="btn primary" onClick={() => setStep(3)}>{models.length ? "Continue" : "Skip for now"}</button>
          </div>
        </Card>
      )}

      {step === 3 && (
        <Card title="Three safety basics" icon={ShieldCheck}>
          <div className="status-list">
            <div className="status-item"><div className="si-icon"><Octagon size={16} /></div><div className="grow"><b>Stop any time</b><div className="small muted">Press <span className="kbd">{settings.stop_shortcut}</span>, the Stop button on the floating bar, or the tray menu. Moving the pointer into the top-left corner pauses.</div></div></div>
            <div className="status-item"><div className="si-icon"><Hand size={16} /></div><div className="grow"><b>Your turn at logins</b><div className="small muted">Passwords, two-step codes and CAPTCHAs are always handed to you. The agent never reads or solves them.</div></div></div>
            <div className="status-item"><div className="si-icon"><ShieldCheck size={16} /></div><div className="grow"><b>Fixed safety gates</b><div className="small muted">Sending, paying, deleting outside the undo area, system settings, shell commands and installs always need your OK. File changes can be undone for {settings.undo_hours} hours.</div></div></div>
          </div>
          <div className="row between" style={{ marginTop: 20 }}>
            <button className="btn" onClick={() => setStep(2)}>Back</button>
            <button className="btn primary" onClick={onDone}>Start using {settings.app_name}</button>
          </div>
        </Card>
      )}

      {adding && <AddProvider onClose={() => setAdding(false)} onAdded={async (id) => { setAdding(false); const p = (await api.listProviders()).find((x) => x.id === id); if (p) setBrowse(p); }} />}
      {browse && <BrowseModels p={browse} onClose={() => { setBrowse(null); refreshModels(); }} />}
    </div>
  );
}

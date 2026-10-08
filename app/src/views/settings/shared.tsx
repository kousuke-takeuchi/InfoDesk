import { useCallback, useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { api } from "../../api";
import { errorMessage, type Settings } from "../../types";

export interface SelectOption { id: string; label: string }
/** 操作の結果。error は失敗したときのバックエンドの文言 (多重実行で見送ったときは ok: false, error: null)。 */
export interface ActionResult { ok: boolean; error: string | null }

/**
 * 保存済みの値が選択肢に無いとき、その値を選択肢の末尾に足す。
 * こうしないと select は先頭の選択肢を表示し、保存されていない値を保存済みのように見せてしまう。
 */
export function withStoredOption(options: SelectOption[], stored: string, labelFor: (v: string) => string = (v) => v): SelectOption[] {
  if (options.some((o) => o.id === stored)) return options;
  return [...options, { id: stored, label: stored === "" ? "未設定" : labelFor(stored) }];
}

/**
 * 設定タブ共通の状態。loadError (get_settings の失敗) と actionError (保存や操作の失敗) は別々に持つ。
 * actionError は次の操作が成功したときだけ消え、再取得の成否では変わらない。
 */
export function useSettingsState() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const inFlight = useRef(new Set<string>());

  const load = useCallback(
    () => api.getSettings().then((s) => { setSettings(s); setLoadError(null); }).catch((e) => setLoadError(errorMessage(e))),
    [],
  );
  useEffect(() => { void load(); }, [load]);

  /** name ごとに同時実行を1件に制限する。成功したら actionError を消して設定を再取得し、結果を返す。 */
  const attempt = useCallback((name: string, fn: () => Promise<unknown>): Promise<ActionResult> => {
    if (inFlight.current.has(name)) return Promise.resolve({ ok: false, error: null });
    inFlight.current.add(name);
    setBusy(new Set(inFlight.current));
    return fn()
      .then(() => { setActionError(null); return load().then(() => ({ ok: true, error: null })); })
      .catch((e) => { const error = errorMessage(e); setActionError(error); return { ok: false, error }; })
      .finally(() => { inFlight.current.delete(name); setBusy(new Set(inFlight.current)); });
  }, [load]);

  const act = useCallback((name: string, fn: () => Promise<unknown>) => attempt(name, fn).then((r) => r.ok), [attempt]);
  const save = useCallback((key: string, value: string) => attempt(`set:${key}`, () => api.setSetting(key, value)), [attempt]);
  return { settings, loadError, actionError, busy, act, save };
}

/**
 * IME の変換を確定する Enter かどうか。WKWebView では確定の Enter が isComposing === false,
 * keyCode === 229 で届くことがあるので両方を見る。
 */
export function isImeEnter(e: KeyboardEvent): boolean {
  return e.nativeEvent.isComposing || e.keyCode === 229;
}

/**
 * blur または Enter で保存するテキスト欄。
 * validate が文言を返したら保存せず欄の横に表示する。normalize は保存前の整形。
 * 保存に失敗しても入力値は残す (value が変わったときだけ draft を差し替える)。
 * 保存の失敗は欄の横にも出し、この欄の保存が成功するか入力値を保存済みの値に戻すまで残す
 * (上部の actionError は別の操作の成功で消えるため、それだけでは失敗した欄が分からなくなる)。
 */
export function SettingField({ id, label, value, onSave, hint, type = "text", width = 260, validate, normalize }: {
  id: string; label: string; value: string; onSave: (v: string) => Promise<ActionResult>; hint?: string; type?: string; width?: number;
  validate?: (v: string) => string | null; normalize?: (v: string) => string;
}) {
  const [draft, setDraft] = useState(value);
  const [invalid, setInvalid] = useState<string | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const sent = useRef<string | null>(null);
  // 保存済みの値が変わったときだけ、レンダー中に draft を差し替える。
  // effect で差し替えると、マウント直後の入力のあとに effect が走って入力を上書きすることがある。
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setDraft(value);
    setInvalid(null);
    setFailed(null);
  }
  // ref はレンダー中に書けないので layout effect で消す。コミット内で同期に走るので、入力のあとに回ることはない
  useLayoutEffect(() => { sent.current = null; }, [value]);
  const commit = () => {
    if (draft === value) { setInvalid(null); setFailed(null); return; }
    const problem = validate?.(draft) ?? null;
    if (problem) { setInvalid(problem); return; }
    setInvalid(null);
    const next = normalize ? normalize(draft) : draft;
    setDraft(next);
    if (next === value) { setFailed(null); return; }
    // Enter の直後に blur でも commit が走るため、送信中または送信済みの同じ値は再送しない。失敗したら再試行できるよう戻す
    if (next === sent.current) return;
    sent.current = next;
    void onSave(next).then((r) => {
      if (sent.current !== next) return; // 送信後に入力が変わった。結果はその新しい入力のものではない
      if (r.ok) { setFailed(null); return; }
      sent.current = null;
      if (r.error !== null) setFailed(r.error);
    });
  };
  const errorId = `${id}-error`;
  return (
    <div className="setting-row">
      <div className="label"><label htmlFor={id}>{label}</label>{hint && <span className="hint">{hint}</span>}</div>
      {invalid && <span className="cell-status error" role="alert">{invalid}</span>}
      {/* 保存の失敗は上部の ErrorLine が読み上げるので、ここは role="alert" にせず欄の説明として結び付ける */}
      {!invalid && failed && <span id={errorId} className="cell-status error">{failed}</span>}
      <input id={id} type={type} className="field" style={{ width }} value={draft} aria-invalid={invalid || failed ? true : undefined}
        aria-describedby={!invalid && failed ? errorId : undefined}
        onChange={(e) => { setDraft(e.target.value); sent.current = null; if (e.target.value === value) setFailed(null); }} onBlur={commit}
        onKeyDown={(e) => { if (e.key === "Enter" && !isImeEnter(e)) commit(); }} />
    </div>
  );
}

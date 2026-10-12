import { useMutation, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { FeedbackKind } from "@/api/contract";
import { api } from "@/api/endpoints";
import { collectDiagnostics } from "@/lib/diagnostics";
import { refreshAll } from "@/lib/queries";
import { capturePage, encodeScreenshot, ScreenshotEditor } from "./screenshot";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";
import { Field, FormError, Textarea } from "./ui/form";
import { Switch } from "./ui/switch";
import { notify } from "./ui/toast";

const MAX_TEXT = 5000;

export const kindText: Record<FeedbackKind, string> = { bug: "Bug", idea: "Idea", question: "Question" };

/** What goes along with the text, in the words of the Security page. */
function DiagnosticsList({ version }: { version: string }) {
  const d = collectDiagnostics(version);
  const rows: [string, string][] = [
    ["Page", d.page],
    ["App version", d.app_version],
    ["Browser", d.browser],
    ["Screen size", d.screen],
    ["Last error messages", d.errors.length ? d.errors.join(" · ") : "none"],
    ["Last request IDs", d.request_ids.length ? `${String(d.request_ids.length)}, such as ${d.request_ids.at(-1) ?? ""}` : "none"],
  ];
  return (
    <details className="rounded-md border border-line px-3 py-2 text-[13px]">
      <summary className="cursor-pointer font-medium">What goes along</summary>
      <dl className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1" data-testid="diagnostics">
        {rows.map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-muted">{label}</dt>
            <dd className="break-words">{value}</dd>
          </div>
        ))}
      </dl>
      <p className="mt-2 text-muted">Never what a request contained.</p>
    </details>
  );
}

function FeedbackForm({ onDone }: { onDone: () => void }) {
  const health = useQuery({ queryKey: ["health"], queryFn: api.health, staleTime: Infinity });
  const version = health.data?.version ?? "";
  const [kind, setKind] = useState<FeedbackKind>("bug");
  const [text, setText] = useState("");
  const [withScreenshot, setWithScreenshot] = useState(false);
  const [source, setSource] = useState<HTMLCanvasElement | null>(null);
  const [edited, setEdited] = useState<HTMLCanvasElement | null>(null);
  const capture = useMutation({ mutationFn: capturePage, onSuccess: setSource });
  const send = useMutation({
    mutationFn: () =>
      api.sendFeedback({
        kind,
        text: text.trim(),
        diagnostics: collectDiagnostics(version),
        screenshot: withScreenshot && edited ? encodeScreenshot(edited) : null,
      }),
    onSuccess: async () => {
      await refreshAll();
      notify("Thank you", "Your report and its answer are under Feedback.");
      onDone();
    },
  });
  return (
    <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); send.mutate(); }}>
      <fieldset>
        <legend className="mb-1 text-[13px] font-medium text-ink-soft">Kind</legend>
        <div className="grid grid-cols-3 gap-2">
          {(Object.keys(kindText) as FeedbackKind[]).map((k) => (
            <label key={k} className={`flex cursor-pointer items-center justify-center gap-2 rounded-md border px-3 py-2 text-sm ${kind === k ? "border-ink bg-sheet" : "border-line-strong"}`}>
              <input type="radio" name="kind" value={k} checked={kind === k} onChange={() => setKind(k)} className="accent-[var(--tc-ink)]" />
              {kindText[k]}
            </label>
          ))}
        </div>
      </fieldset>
      <Field label="What happened, or what you would like" hint={`${String(text.length)} of ${String(MAX_TEXT)} characters`}>
        <Textarea required rows={5} maxLength={MAX_TEXT} value={text} onChange={(e) => setText(e.target.value)} />
      </Field>
      <div>
        <label className="flex items-center gap-3 text-sm">
          <Switch
            label="Add a screenshot"
            checked={withScreenshot}
            onCheckedChange={(on) => {
              setWithScreenshot(on);
              if (on && !source) capture.mutate();
            }}
          />
          Add a screenshot of this page
        </label>
        {withScreenshot ? (
          <div className="mt-3">
            {capture.isPending ? <p className="text-sm text-muted">Taking the screenshot…</p> : null}
            <FormError error={capture.error} />
            {source ? <ScreenshotEditor source={source} onChange={setEdited} /> : null}
          </div>
        ) : null}
      </div>
      <DiagnosticsList version={version} />
      <FormError error={send.error} />
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Link to="/feedback" onClick={onDone} className="text-sm text-accent hover:underline">
          Your earlier reports
        </Link>
        <div className="flex gap-2">
          <Button onClick={onDone}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!text.trim() || send.isPending || (withScreenshot && !edited)}>
            Send
          </Button>
        </div>
      </div>
    </form>
  );
}

/** The feedback panel. The form starts fresh each time it opens. */
export function FeedbackDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Send feedback" description="A bug, an idea or a question. You see the whole report before you send it." wide>
      {open ? <FeedbackForm onDone={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

/** A button that opens the feedback panel. */
export function FeedbackButton({ children, className }: { children: ReactNode; className?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" onClick={() => setOpen(true)} className={className}>
        {children}
      </button>
      <FeedbackDialog open={open} onOpenChange={setOpen} />
    </>
  );
}

import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Bill, BillCloud, BillCsvBill, BillCsvProblem } from "@/api/contract";
import { api, type BillCsvFile } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { downloadFile, toCsv } from "@/lib/csv";
import { formatRange } from "@/lib/format";
import { fromDecimal, nanosOf } from "@/lib/money";
import { refreshAll } from "@/lib/queries";

// Docs: Bills and reconciliation > CSV. Any bill can be uploaded as CSV: our
// own columns (the bill schema), or the cost export of Bedrock, Vertex or
// Foundry as one month's bill. The upload shows what it read before anything
// is saved; a file with a problem saves nothing.

type Kind = "" | BillCloud;

const kinds: { value: Kind; label: string; hint: string }[] = [
  { value: "", label: "Bills in the bill schema's columns", hint: "A header naming provider, from, to, amount and currency, optionally exchange_rate, then one row per bill." },
  { value: "bedrock", label: "AWS Cost and Usage Report, for Bedrock", hint: "The report as AWS exports it. Amazon Bedrock usage inside the month is the bill." },
  { value: "vertex", label: "Google Cloud billing export, for Vertex AI", hint: "The cost table download or a BigQuery export saved as CSV. Vertex AI usage inside the month is the bill." },
  { value: "foundry", label: "Azure cost export, for Foundry", hint: "The export from Cost Management. Foundry usage inside the month is the bill." },
];

const isMonth = (text: string) => /^\d{4}-(0[1-9]|1[0-2])$/.test(text);

/** The bill schema's columns with one example row, to fill in. */
function downloadTemplate(): void {
  downloadFile("bills.csv", toCsv(["provider", "from", "to", "amount", "currency", "exchange_rate"], [["anthropic", "2026-03-01", "2026-03-31", "1000.00", "USD", ""]]));
}

/** How a bill in the file stands against the bill in force for its provider and range. */
type Standing = { kind: "new" } | { kind: "same" } | { kind: "restates"; amount: string; currency: string };

function standingOf(bill: BillCsvBill, inForce: Bill[]): Standing {
  const current = inForce.find((b) => b.provider === bill.provider && b.from === bill.from && b.to === bill.to && !b.reversed_by);
  if (!current) return { kind: "new" };
  const same = current.currency === bill.currency && nanosOf(fromDecimal(current.amount, current.currency)) === nanosOf(fromDecimal(bill.amount, bill.currency));
  return same ? { kind: "same" } : { kind: "restates", amount: current.amount, currency: current.currency };
}

function StandingCell({ standing }: { standing: Standing }) {
  if (standing.kind === "new") return <Badge tone="accent">new bill</Badge>;
  if (standing.kind === "same") return <Badge>same figures, nothing changes</Badge>;
  return (
    <span className="flex flex-wrap items-center gap-1.5">
      <Badge tone="ink">restates</Badge>
      <span className="text-muted line-through"><Amount value={standing.amount} currency={standing.currency} /></span>
    </span>
  );
}

/** The problem the service found, with the rows of the file around it. */
function Problem({ problem, text }: { problem: BillCsvProblem; text: string }) {
  const where = [problem.row ? `Row ${String(problem.row)}` : null, problem.column ? `column ${problem.column}` : null].filter(Boolean).join(", ");
  const lines = text.replace(/^\uFEFF/, "").split(/\r\n|\n|\r/);
  const row = problem.row ?? null;
  const shown = row === null ? [] : [1, row - 1, row].filter((n, i, all) => n >= 1 && n <= lines.length && all.indexOf(n) === i);
  return (
    <div role="alert" className="rounded-lg border border-exception/30 bg-exception-tint px-4 py-3 text-sm">
      <p className="text-exception">
        {where ? <span className="font-medium">{where}: </span> : null}
        {problem.message}
      </p>
      <p className="mt-1 text-ink-soft">Fix the file and choose it again. Nothing was saved.</p>
      {shown.length ? (
        <ol className="mt-3 overflow-x-auto rounded-md bg-page font-mono text-xs">
          {shown.map((n) => (
            <li key={n} className={`flex gap-3 px-3 py-1 ${n === row ? "bg-exception/10 text-exception" : "text-muted"}`}>
              <span className="w-6 shrink-0 select-none text-right">{n}</span>
              <span className="whitespace-pre">{lines[n - 1]}</span>
            </li>
          ))}
        </ol>
      ) : null}
    </div>
  );
}

/** A dashed box that takes a CSV file by click or drop. */
function FileDrop({ file, onFile }: { file: File | null; onFile: (file: File | null) => void }) {
  const [over, setOver] = useState(false);
  return (
    <label
      className={`flex cursor-pointer flex-col items-center justify-center gap-1 rounded-lg border border-dashed px-6 py-7 text-center transition-colors ${over ? "border-accent bg-accent-tint" : "border-line-strong hover:bg-sheet"}`}
      onDragOver={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        e.preventDefault();
        setOver(false);
        onFile(e.dataTransfer.files.item(0));
      }}
    >
      <input type="file" accept=".csv,text/csv" className="sr-only" aria-label="CSV file" onChange={(e) => onFile(e.target.files?.item(0) ?? null)} />
      {file ? (
        <>
          <span className="text-sm font-medium">{file.name}</span>
          <span className="text-xs text-muted">{formatSize(file.size)}. Choose another file to replace it.</span>
        </>
      ) : (
        <>
          <span className="text-sm font-medium">Choose a CSV file</span>
          <span className="text-xs text-muted">or drop it here</span>
        </>
      )}
    </label>
  );
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${String(bytes)} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function UploadCsvDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }): ReactNode {
  const [kind, setKind] = useState<Kind>("");
  const [month, setMonth] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const options: BillCsvFile = kind ? { cloud: kind, month } : {};
  const ready = file !== null && (kind === "" || isMonth(month));

  const inForce = useQuery({ queryKey: ["bills"], queryFn: api.bills, enabled: open });
  const preview = useQuery({
    queryKey: ["bill-csv-preview", kind, month, file?.name, file?.size, file?.lastModified],
    enabled: ready,
    staleTime: Infinity,
    queryFn: async () => {
      if (!file) throw new Error("Choose a file first.");
      const text = await file.text();
      return { text, read: await api.previewBillCsv(options, text) };
    },
  });
  const bills = preview.data?.read.bills ?? [];
  const problem = preview.data?.read.problem ?? null;

  const close = () => {
    onOpenChange(false);
    setFile(null);
    setMonth("");
    setKind("");
  };
  const upload = useMutation({
    mutationFn: () => {
      if (!preview.data) throw new Error("Choose a file first.");
      return api.uploadBillCsv(options, preview.data.text);
    },
    onSuccess: async (stored) => {
      await refreshAll();
      const count = stored.bills.length;
      const same = count - stored.changed;
      notify(
        `${String(count)} bill${count === 1 ? "" : "s"} uploaded`,
        `${String(stored.changed)} new bill event${stored.changed === 1 ? "" : "s"}${same ? `, ${String(same)} already stored with the same figures` : ""}.`,
      );
      close();
    },
  });

  const chosen = kinds.find((k) => k.value === kind) ?? kinds[0];
  const standings = new Map(bills.map((b) => [`${b.provider} ${b.from} ${b.to}`, standingOf(b, inForce.data ?? [])] as const));
  const changes = [...standings.values()].filter((s) => s.kind !== "same").length;

  return (
    <Dialog open={open} onOpenChange={(next) => (next ? onOpenChange(true) : close())} title="Upload a CSV bill" description="What the file holds is shown before anything is saved. A file with a problem saves nothing." wide>
      <div className="grid gap-4">
        <div className="grid gap-3 sm:grid-cols-[1fr_auto] sm:items-start">
          <Field label="The file is" hint={chosen?.hint}>
            <Select
              value={kind}
              onChange={(e) => {
                setKind(e.target.value as Kind);
                setFile(null);
              }}
            >
              {kinds.map((k) => (
                <option key={k.value} value={k.value}>{k.label}</option>
              ))}
            </Select>
          </Field>
          {kind ? (
            <Field label="Month the export bills" className="sm:w-44">
              <Input type="month" value={month} onChange={(e) => setMonth(e.target.value)} />
            </Field>
          ) : (
            <div className="sm:pt-6">
              <Button size="sm" variant="ghost" onClick={downloadTemplate}>Download a template</Button>
            </div>
          )}
        </div>

        <FileDrop file={file} onFile={setFile} />

        {kind && file && !isMonth(month) ? <p className="text-sm text-muted">Give the month the export is the bill for, such as 2026-03.</p> : null}
        {preview.isFetching ? <Loading /> : null}
        {preview.error ? <ErrorBox error={preview.error} /> : null}
        {problem && preview.data ? <Problem problem={problem} text={preview.data.text} /> : null}
        {bills.length ? (
          <DataTable
            caption="What the file holds"
            stack
            rows={bills}
            rowKey={(b) => `${b.provider} ${b.from} ${b.to}`}
            columns={[
              { key: "provider", header: "Provider", cell: (b) => b.provider },
              { key: "range", header: "Range", cell: (b) => formatRange(b.from, b.to) },
              { key: "amount", header: "Amount", align: "right", cell: (b) => <Amount value={b.amount} currency={b.currency} /> },
              { key: "rate", header: "Exchange rate", align: "right", cell: (b) => (b.exchange_rate ? <span className="num">{b.exchange_rate}</span> : <span className="text-muted">filled in</span>) },
              ...(kind ? [{ key: "lines", header: "Lines summed", align: "right" as const, cell: (b: BillCsvBill) => <span className="num">{String(b.lines)}</span> }] : []),
              { key: "standing", header: "Against the bill in force", cell: (b) => <StandingCell standing={standings.get(`${b.provider} ${b.from} ${b.to}`) ?? { kind: "new" }} /> },
            ]}
          />
        ) : null}

        <FormError error={upload.error} />
        <div className="flex flex-wrap items-center justify-end gap-2 border-t border-line pt-4">
          {bills.length && changes === 0 ? <span className="mr-auto text-sm text-muted">Every bill is already stored with the same figures.</span> : null}
          <Button variant="ghost" onClick={close}>Cancel</Button>
          <Button variant="primary" disabled={!bills.length || upload.isPending} onClick={() => upload.mutate()}>
            {bills.length > 1 ? `Upload ${String(bills.length)} bills` : "Upload the bill"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

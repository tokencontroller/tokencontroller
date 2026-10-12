import type { ReactNode } from "react";
import { downloadFile, toCsv } from "@/lib/csv";
import { Button } from "./button";

export interface Column<Row> {
  key: string;
  header: string;
  cell: (row: Row) => ReactNode;
  /** The value in the CSV download. Without it the column is left out of the CSV. */
  csv?: (row: Row) => string | number | null;
  align?: "left" | "right";
  /** Hide on narrow screens (below md, or below xl); the CSV still has it. */
  wide?: boolean | "xl";
  /** Only in the CSV download, not on screen. */
  csvOnly?: boolean;
  className?: string;
}

export function DataTable<Row>({
  columns,
  rows,
  rowKey,
  csvName,
  totals,
  empty,
  onRowClick,
  rowClassName,
  caption,
  stack = false,
}: {
  columns: Column<Row>[];
  rows: Row[];
  rowKey: (row: Row) => string;
  /** File name of the CSV download, such as `token-entries-2026-09.csv`. */
  csvName?: string;
  /** A total row under a double rule: a label and values by column key. Also the last CSV row. */
  totals?: { label: string; values: Partial<Record<string, ReactNode>>; csv?: Partial<Record<string, string | number>> };
  empty?: ReactNode;
  onRowClick?: ((row: Row) => void) | undefined;
  rowClassName?: (row: Row) => string;
  caption?: string;
  /** On a phone, show each row as a card with the headers as labels, instead of scrolling sideways. */
  stack?: boolean;
}) {
  const exportable = columns.filter((c) => c.csv);
  const download = () => {
    if (!csvName) return;
    const csv = toCsv(
      exportable.map((c) => c.header || c.key),
      [
        ...rows.map((row) => exportable.map((c) => c.csv?.(row) ?? null)),
        ...(totals ? [exportable.map((c, i) => (i === 0 ? totals.label : (totals.csv?.[c.key] ?? null)))] : []),
      ],
    );
    downloadFile(csvName, csv);
  };
  const visible = columns.filter((c) => !c.csvOnly);
  const hide = (c: Column<Row>) => (c.wide === "xl" ? "hidden xl:table-cell" : c.wide ? "hidden md:table-cell" : "");
  // Stacked rows: blocks below sm, a table from sm up.
  const st = stack
    ? {
        table: "max-sm:block",
        head: "max-sm:hidden",
        body: "max-sm:block",
        row: "max-sm:block max-sm:py-2",
        cell: "max-sm:flex max-sm:px-4 max-sm:items-start max-sm:justify-between max-sm:gap-4 max-sm:py-1 max-sm:text-right max-sm:before:shrink-0 max-sm:before:pt-0.5 max-sm:before:text-left max-sm:before:font-mono max-sm:before:text-[11px] max-sm:before:uppercase max-sm:before:tracking-wide max-sm:before:text-muted max-sm:before:content-[attr(data-label)]",
      }
    : { table: "", head: "", body: "", row: "", cell: "" };
  return (
    <div>
      {csvName ? (
        <div className="mb-2 flex justify-end">
          <Button size="sm" variant="ghost" onClick={download} disabled={!rows.length}>
            <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><path d="M8 2v8m0 0L5 7m3 3l3-3M3 13h10" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /></svg>
            Download CSV
          </Button>
        </div>
      ) : null}
      <div className="-mx-4 overflow-x-auto sm:mx-0">
        <table className={`w-full min-w-full border-collapse text-sm ${st.table}`}>
          {caption ? <caption className="sr-only">{caption}</caption> : null}
          <thead className={st.head}>
            <tr className="border-b border-ink">
              {visible.map((c) => (
                <th
                  key={c.key}
                  scope="col"
                  className={`whitespace-nowrap px-2 py-2 font-mono text-[11px] font-medium uppercase tracking-wide text-muted first:pl-4 last:pr-4 sm:first:pl-2 sm:last:pr-2 ${c.align === "right" ? "text-right" : "text-left"} ${hide(c)} ${c.className ?? ""}`}
                >
                  {c.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className={st.body}>
            {rows.length === 0 ? (
              <tr>
                <td colSpan={visible.length} className="px-4 py-8 text-center text-sm text-muted">
                  {empty ?? "Nothing here."}
                </td>
              </tr>
            ) : (
              rows.map((row) => (
                <tr
                  key={rowKey(row)}
                  onClick={onRowClick ? () => onRowClick(row) : undefined}
                  className={`border-b border-line align-top ${st.row} ${onRowClick ? "cursor-pointer hover:bg-sheet" : ""} ${rowClassName?.(row) ?? ""}`}
                >
                  {visible.map((c) => (
                    <td
                      key={c.key}
                      data-label={c.header}
                      className={`${c.wide ? "" : st.cell} px-2 py-2.5 first:pl-4 last:pr-4 sm:first:pl-2 sm:last:pr-2 ${c.align === "right" ? "num whitespace-nowrap" : ""} ${hide(c)} ${c.className ?? ""}`}
                    >
                      {c.cell(row)}
                    </td>
                  ))}
                </tr>
              ))
            )}
          </tbody>
          {totals && rows.length ? (
            <tfoot className={`border-t-[3px] border-double border-ink font-medium ${st.body}`}>
              <tr className={st.row}>
                {visible.map((c, i) => (
                  <td key={c.key} data-label={i === 0 ? "" : c.header} className={`${c.wide ? "" : st.cell} whitespace-nowrap px-2 py-2.5 first:pl-4 last:pr-4 sm:first:pl-2 sm:last:pr-2 ${c.align === "right" ? "num" : ""} ${hide(c)}`}>
                    {i === 0 ? totals.label : totals.values[c.key]}
                  </td>
                ))}
              </tr>
            </tfoot>
          ) : null}
        </table>
      </div>
    </div>
  );
}

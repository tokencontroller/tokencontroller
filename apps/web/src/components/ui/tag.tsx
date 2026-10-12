import type { Tags } from "@/api/contract";
import { tagList } from "@/lib/format";
import { usePersonName } from "@/lib/queries";

// Tags carry a spectrum color per tag name, so the eye finds the ticket or
// the client in a row. Color is used here because it means something.
const fixed: Record<string, number> = { ticket: 6, project: 5, team: 4, client: 7, purpose: 2, person: 3 };

export function tagColor(name: string): string {
  let slot = fixed[name];
  if (!slot) {
    let hash = 0;
    for (const char of name) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
    slot = (hash % 7) + 1;
  }
  return `var(--color-spectrum-${slot})`;
}

export function Tag({ name, value, onRemove }: { name: string; value: string; onRemove?: () => void }) {
  // The person tag holds a person's id; people read their name.
  const personName = usePersonName();
  const shown = name === "person" ? personName(value) : value;
  return (
    <span className="inline-flex max-w-full items-center gap-1.5 rounded-sm border border-line bg-page py-px pl-1.5 pr-2 text-xs leading-5 text-ink shadow-chip">
      <span aria-hidden="true" className="size-2 shrink-0 rounded-full" style={{ background: tagColor(name) }} />
      <span className="text-muted">{name}</span>
      <span className="truncate font-medium">{shown}</span>
      {onRemove ? (
        <button type="button" onClick={onRemove} aria-label={`Remove ${name}`} className="-mr-1 ml-0.5 rounded-sm px-0.5 text-muted hover:text-ink">
          ×
        </button>
      ) : null}
    </span>
  );
}

export function TagRow({ tags, hide = ["person"] }: { tags: Tags; hide?: string[] }) {
  const list = tagList(tags).filter(([name]) => !hide.includes(name));
  if (!list.length) return <span className="text-xs text-muted">no tags</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {list.map(([name, value]) => (
        <Tag key={name} name={name} value={value} />
      ))}
    </span>
  );
}

import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { api } from "@/api/endpoints";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";
import { Field, FormError, Textarea } from "./ui/form";

const MAX_MESSAGE = 5000;

function SupportForm({ answerTo, onDone }: { answerTo: string; onDone: () => void }) {
  const [message, setMessage] = useState("");
  const send = useMutation({ mutationFn: () => api.sendSupportMessage(message.trim()) });
  if (send.isSuccess) {
    return (
      <div className="space-y-4">
        <p className="text-sm" data-testid="support-sent">
          Thank you. Your message is in. The answer comes to <b>{answerTo}</b>.
        </p>
        <div className="flex justify-end">
          <Button variant="primary" onClick={onDone}>
            Done
          </Button>
        </div>
      </div>
    );
  }
  return (
    <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); send.mutate(); }}>
      <Field label="Message" hint={`The answer comes by email to ${answerTo}.`}>
        <Textarea required rows={6} maxLength={MAX_MESSAGE} value={message} onChange={(e) => setMessage(e.target.value)} />
      </Field>
      <p className="text-[13px] text-muted">
        An AI assistant answers from the docs within minutes, and every answer says so. What the docs do not answer goes to Jan Beck, who runs Token Controller and answers in person.
      </p>
      <FormError error={send.error} />
      <div className="flex justify-end gap-2">
        <Button onClick={onDone}>Cancel</Button>
        <Button type="submit" variant="primary" disabled={!message.trim() || send.isPending}>
          Send
        </Button>
      </div>
    </form>
  );
}

/** "Support" in the menu: one message, answered by email. Only where support is on. */
export function SupportDialog({ answerTo, open, onOpenChange }: { answerTo: string; open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Write to support" description="Ask about Token Controller, or tell us what is missing.">
      {open ? <SupportForm answerTo={answerTo} onDone={() => onOpenChange(false)} /> : null}
    </Dialog>
  );
}

import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { api } from "@/api/endpoints";
import { Button } from "@/components/ui/button";
import { ErrorBox, Loading, PageHeader } from "@/components/ui/display";
import { formatDateTime } from "@/lib/format";
import { useMe } from "@/lib/queries";

const cliRoute = getRouteApi("/app/cli/$code");

/** The page `tc connect` opens: the person checks the code and connects this computer. */
export function ConnectComputerPage(): ReactNode {
  const { code } = cliRoute.useParams();
  const me = useMe();
  const authorization = useQuery({ queryKey: ["cli", code], queryFn: () => api.cliAuthorization(code) });
  const confirm = useMutation({ mutationFn: () => api.confirmCli(code) });
  const decline = useMutation({ mutationFn: () => api.declineCli(code) });
  const status = decline.data?.status ?? confirm.data?.status ?? authorization.data?.status;
  return (
    <div className="max-w-lg">
      <PageHeader title="Connect a computer" lead="The tc command line asked to connect to your account. It reads agent sessions on that computer and submits your token sheet." />
      {authorization.isPending ? <Loading /> : authorization.isError ? <ErrorBox error={authorization.error} /> : (
        <div className="rounded-lg border border-line p-5">
          <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-sm">
            <dt className="text-muted">Code</dt>
            <dd className="font-mono font-medium tracking-wider" data-testid="cli-code">{authorization.data.user_code}</dd>
            <dt className="text-muted">Computer</dt>
            <dd className="font-mono">{authorization.data.device}</dd>
            <dt className="text-muted">Organization</dt>
            <dd>{authorization.data.organization}</dd>
            <dt className="text-muted">You</dt>
            <dd>{me.person.name}</dd>
          </dl>
          <div className="mt-5 border-t border-line pt-4" data-testid="cli-status">
            {status === "pending" ? (
              <>
                <p className="mb-4 text-sm">Check that the terminal shows the same code. Connect only a computer you use yourself.</p>
                <div className="flex gap-2">
                  <Button variant="primary" disabled={confirm.isPending} onClick={() => confirm.mutate()}>
                    Connect this computer
                  </Button>
                  <Button disabled={decline.isPending} onClick={() => decline.mutate()}>
                    Cancel
                  </Button>
                </div>
              </>
            ) : status === "confirmed" ? (
              <p className="text-sm text-posted">Connected. Go back to your terminal; tc finishes on its own. You can remove this computer under Settings.</p>
            ) : status === "declined" ? (
              <p className="text-sm">Not connected. Close this page.</p>
            ) : (
              <p className="text-sm text-exception">
                This code expired at {formatDateTime(authorization.data.expires_at)}. Run <code className="font-mono">tc connect</code> again.
              </p>
            )}
          </div>
        </div>
      )}
      <Link to="/" className="mt-6 inline-block text-sm text-accent hover:underline">
        Back to the overview
      </Link>
    </div>
  );
}

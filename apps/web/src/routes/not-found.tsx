import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";

export function NotFoundPage(): ReactNode {
  return (
    <div className="mx-auto max-w-md px-4 py-20 text-center">
      <h1 className="text-lg font-semibold">This page does not exist</h1>
      <Link to="/" className="mt-3 inline-block text-sm text-accent hover:underline">
        Go to the overview
      </Link>
    </div>
  );
}

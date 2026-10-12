// Generates the API types from the service's OpenAPI document (ADR 0009).
// The file is not committed: CI generates it before lint, typecheck, test and
// build, so a change to the API that the web app does not follow fails there.
// TC_OPENAPI points at another copy of the document, such as one from a
// service branch that has not merged yet; CI never sets it.
import { writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import openapiTS, { astToString } from "openapi-typescript";

const source = process.env.TC_OPENAPI
  ? pathToFileURL(process.env.TC_OPENAPI)
  : new URL("../../server/openapi.json", import.meta.url);
const target = new URL("../src/api/generated.ts", import.meta.url);

const ast = await openapiTS(source);
const banner = "// Generated from apps/server/openapi.json by scripts/generate-api.mjs. Do not edit.\n\n";
writeFileSync(target, banner + astToString(ast));

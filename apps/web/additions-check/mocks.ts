// The check slot's mock answers (index.ts in this folder).

import { http, HttpResponse } from "msw";
import type { AddedMocks } from "@/mocks/handlers";

export const addedMocks: AddedMocks = ({ API, member }) => [http.get(`${API}/plan`, member(({ db }) => HttpResponse.json({ name: db.organization.name })))];

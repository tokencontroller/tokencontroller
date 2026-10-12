import { expect, test } from "vitest";
import { parseCsv, toCsv } from "./csv";

test("CSV quotes what needs quoting and reads it back", () => {
  const rows = [
    ["KD-214", "Fixed checkout loop", "41.87"],
    ["KD-231", 'Tuned "search", ranking', "88.30"],
    ["", "Two\nlines", null],
  ];
  const text = toCsv(["ticket", "summary", "cost"], rows);
  expect(text.split("\r\n")[0]).toBe("ticket,summary,cost");
  expect(parseCsv(text)).toEqual([
    ["ticket", "summary", "cost"],
    ["KD-214", "Fixed checkout loop", "41.87"],
    ["KD-231", 'Tuned "search", ranking', "88.30"],
    ["", "Two\nlines", ""],
  ]);
});

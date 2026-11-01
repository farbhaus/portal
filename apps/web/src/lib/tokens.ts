// The tokens an upload link's subfolder template can use; the API renders them when a client's
// session starts (src/services/upload_paths.rs).

export type Token = { name: string; desc: string };

export const UPLOAD_TOKENS: Token[] = [
  { name: "date", desc: "YYYY-MM-DD" },
  { name: "year", desc: "YYYY" },
  { name: "month", desc: "MM" },
  { name: "day", desc: "DD" },
  { name: "time", desc: "24-hour time, HHhMM (09h30)" },
  { name: "uploader_name", desc: "Name entered by the uploader" },
];

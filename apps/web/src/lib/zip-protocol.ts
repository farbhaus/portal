// The contract between the download page and the service worker that streams the ZIP.
//
// Kept in its own file with no runtime dependencies so both sides can import it: the worker is
// bundled separately from the app and must not pull DOM code in through the back door.

/** Everything under here is the worker's; scoped to /d/ so admin pages never route through it. */
export const ZIP_PREFIX = "/d/_zip/";

/** Firefox kills a worker that is still feeding a download unless the page keeps poking it. */
export const KEEPALIVE_PATH = `${ZIP_PREFIX}keep-alive`;

// The page checks it can really reach the worker by loading this in a hidden iframe and looking for
// the title below. It has to be an iframe navigation, not a fetch: that is the exact channel a
// download uses, and the two do not always agree (see zipdownload.ts).
export const PROBE_PATH = `${ZIP_PREFIX}probe`;
export const PROBE_TITLE = "portal-zip-ok";

/** One file in the archive. `name` is the full in-zip path, already de-duplicated by the page. */
export type ZipEntry = { id: string; name: string; size: number | null };

/** A download the page has asked the worker to assemble. */
export type ZipJob = {
  jobId: string;
  token: string;
  sessionId: string;
  entries: ZipEntry[];
};

/** page → worker */
export type ZipJobMessage = { type: "portal-zip-job" } & ZipJob;
export type ZipCancelMessage = { type: "portal-zip-cancel"; jobId: string };

/** worker → page */
export type ZipProgressMessage = {
  type: "portal-zip-progress";
  jobId: string;
  fileIndex: number;
  fileName: string;
  filesTotal: number;
  bytesDone: number;
  bytesTotal: number | null;
};

export type ZipDoneMessage = {
  type: "portal-zip-done";
  jobId: string;
  saved: number;
  failed: string[];
};

export type ZipErrorMessage = {
  type: "portal-zip-error";
  jobId: string;
  message: string;
};

/** Sent whether the recipient cancelled from our page or from the browser's download manager. */
export type ZipCancelledMessage = { type: "portal-zip-cancelled"; jobId: string };

export type ZipWorkerMessage =
  | ZipProgressMessage
  | ZipDoneMessage
  | ZipErrorMessage
  | ZipCancelledMessage;

import { redirect } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";

export type PathSegment = { id: string; name: string };

// A sync rule covering this destination's folder (exact, or via a recursive parent rule).
export type DestinationSyncRule = {
  id: string;
  name: string;
  destination_path: string;
  enabled: boolean;
  recursive: boolean;
  exact: boolean;
};

export type DestinationUploadLink = {
  id: string;
  label: string;
  state: string;
};

export type DestinationConfig = {
  type?: string;
  account_id?: string;
  workspace_id?: string;
  project_id?: string;
  folder_id?: string;
  folder_name?: string;
  project_name?: string;
  // Cached Frame.io breadcrumb, root→leaf (may be absent on legacy rows).
  path?: PathSegment[];
};

export type Destination = {
  id: string;
  display_name: string;
  config: DestinationConfig;
  subtitle: string | null;
  sync_rules: DestinationSyncRule[];
  upload_links: DestinationUploadLink[];
  created_at: string;
  updated_at: string;
};

export const load: PageServerLoad = async () => {
  throw redirect(302, "/sync");
};

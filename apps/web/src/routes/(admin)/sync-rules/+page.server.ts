import { redirect } from "@sveltejs/kit";
import type { PageServerLoad } from "./$types";
import type { PathSegment } from "../destinations/+page.server";

export type SyncRuleSource = {
  type?: string;
  account_id?: string;
  workspace_id?: string;
  project_id?: string;
  folder_id?: string;
  folder_name?: string;
  project_name?: string;
  recursive?: boolean;
  // Cached Frame.io breadcrumb, root→leaf (may be absent on legacy rows).
  path?: PathSegment[];
};

export type SyncRule = {
  id: string;
  name: string;
  source: SyncRuleSource;
  destination_path: string;
  conflict_policy: string;
  path_template: string | null;
  enabled: boolean;
  subscription_active: boolean;
  created_at: string;
};

export const load: PageServerLoad = async () => {
  throw redirect(302, "/sync");
};

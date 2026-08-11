import { Routes } from "@angular/router";
import { AdminLoginComponent } from "./admin-login.component";
import { AdminReviewQueueComponent } from "./admin-review-queue.component";
import { AdminShellComponent } from "./admin-shell.component";
import { AdminTransactionExplorerComponent } from "./admin-transaction-explorer.component";
import { AdminCampaignsComponent } from "./admin-campaigns.component";
import { AdminEmergencyActionComponent } from "./admin-emergency-action.component";
import { AdminJobsComponent } from "./admin-jobs.component";
import { AdminOperationsOverviewComponent } from "./admin-operations-overview.component";
import { AdminResourceListComponent } from "./admin-resource-list.component";
import { AdminHighRiskActionComponent } from "./admin-high-risk-action.component";

export const routes: Routes = [
  { path: "login", component: AdminLoginComponent },
  {
    path: "",
    component: AdminShellComponent,
    children: [
      { path: "", pathMatch: "full", redirectTo: "store-reviews" },
      {
        path: "operations",
        component: AdminOperationsOverviewComponent,
      },
      { path: "store-reviews", component: AdminReviewQueueComponent },
      {
        path: "members",
        component: AdminResourceListComponent,
        data: { kind: "members" },
      },
      { path: "transactions", component: AdminTransactionExplorerComponent },
      { path: "campaigns", component: AdminCampaignsComponent },
      {
        path: "campaigns/:id/emergency-action",
        component: AdminEmergencyActionComponent,
      },
      {
        path: "jobs",
        component: AdminJobsComponent,
      },
      {
        path: "notifications",
        component: AdminResourceListComponent,
        data: { kind: "notifications" },
      },
      {
        path: "cases",
        component: AdminResourceListComponent,
        data: { kind: "cases" },
      },
      {
        path: "audit",
        component: AdminResourceListComponent,
        data: { kind: "audit" },
      },
      { path: "high-risk-action", component: AdminHighRiskActionComponent },
    ],
  },
  { path: "**", redirectTo: "store-reviews" },
];

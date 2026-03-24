output "service_url" {
  description = "Public URL of the deployed Cloud Run service."
  value       = google_cloud_run_v2_service.api.uri
}

output "artifact_registry_repository" {
  description = "Full path of the Artifact Registry container repository."
  value       = "${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.containers.repository_id}"
}

output "cloud_run_service_account" {
  description = "Email of the Cloud Run runtime service account."
  value       = google_service_account.cloud_run.email
}

output "github_actions_service_account" {
  description = "Email of the GitHub Actions deployment service account."
  value       = google_service_account.github_actions.email
}

output "workload_identity_provider" {
  description = "Full resource name of the Workload Identity Federation provider (used in GitHub Actions)."
  value       = google_iam_workload_identity_pool_provider.github.name
}

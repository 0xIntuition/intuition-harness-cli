variable "project_id" {
  description = "GCP project ID for resource provisioning."
  type        = string
}

variable "region" {
  description = "GCP region for Artifact Registry and Cloud Run."
  type        = string
  default     = "us-central1"
}

variable "service_name" {
  description = "Name of the Cloud Run service."
  type        = string
  default     = "metastack-api"
}

variable "image_tag" {
  description = "Container image tag to deploy (e.g. latest, v0.1.0, sha-abc1234)."
  type        = string
  default     = "latest"
}

variable "min_instance_count" {
  description = "Minimum number of Cloud Run instances (0 allows scale-to-zero)."
  type        = number
  default     = 0
}

variable "max_instance_count" {
  description = "Maximum number of Cloud Run instances."
  type        = number
  default     = 3
}

variable "cpu" {
  description = "CPU allocation per Cloud Run instance."
  type        = string
  default     = "1"
}

variable "memory" {
  description = "Memory allocation per Cloud Run instance."
  type        = string
  default     = "512Mi"
}

variable "env_vars" {
  description = "Environment variables passed to the Cloud Run service."
  type        = map(string)
  default     = {}
}

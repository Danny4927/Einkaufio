terraform {
  required_version = ">= 1.10.0, < 2.0.0"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.72"
    }
  }

  # The bootstrap root starts with local state (it creates the bucket that
  # will hold remote state). After the first apply, copy backend.tf.example
  # to backend.tf and run `terraform init -migrate-state`; see
  # docs/runbooks/terraform-bootstrap.md.
}

provider "aws" {
  region = var.region

  default_tags {
    tags = {
      Project   = "einkaufio"
      ManagedBy = "terraform"
      Root      = "bootstrap"
    }
  }
}

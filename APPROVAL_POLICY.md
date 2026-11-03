# PR Approval Policy

## Objective

Reduce unnecessary human code reviews by identifying low-risk pull requests that qualify for automatic approval.

## Approval Criteria

A PR may qualify for automatic approval when:

- Cursor Bugbot reports no unresolved issues.
- The change is low risk with a limited blast radius.
- Existing functionality is preserved or the behavioral change is small and well understood.
- Relevant tests and CI checks pass.
- The change does not introduce security, reliability, or compatibility concerns.

## Low-Risk Changes

Examples of changes eligible for approval:

- Documentation, comments, and formatting.
- Small bug fixes with clear, verified behavior.
- Build metadata, changelogs, and release tooling changes.
- Isolated refactoring with no meaningful behavioral changes.
- Minor test improvements.

## Human Review Required

Require human review for:

- Authentication, authorization, or security-sensitive changes.
- Significant architectural or API changes.
- Database migrations or destructive operations.
- Dependency changes introducing security or compatibility risks.
- Changes with unclear behavior, insufficient testing, or substantial blast radius.

## Decision

Approve only when the evidence supports a low-risk classification. When uncertain, require human review.

## Shadow Mode

For this evaluation, do not actually approve, merge, or modify any pull request.

Report whether the PR **WOULD APPROVE**, **REQUIRES REVIEW**, or **UNKNOWN**, with a brief explanation.

Post the assessment as a regular GitHub PR comment.

# School system

**Complexity:** ⭐⭐⭐☆☆
**Models:** 5

## What this domain teaches

Course rosters, term-by-term enrollment, weighted grading, derived
counters. Demonstrates the canonical many-to-many pattern (Student
↔ Course via Enrollment) and an `editable: false` derived field
(`Course.enrolled_count`) that the application keeps in sync.

## Models

| Model       | Key fields                                                                                              | Relations                                              |
|-------------|---------------------------------------------------------------------------------------------------------|--------------------------------------------------------|
| Student     | `student_number`, `first_name`, `last_name`, `email`, `date_of_birth`, `enrollment_year`, `grade_level`, `guardian_name?`, `is_active` | (none)                                |
| Teacher     | `employee_number`, `first_name`, `last_name`, `email`, `department`, `hire_date`, `is_active`           | (none)                                                 |
| Course      | `teacher_id`, `code`, `title`, `credit_hours`, `academic_year`, `term`, `capacity`, `enrolled_count`, `is_active` | belongs_to Teacher                            |
| Enrollment  | `student_id`, `course_id`, `enrolled_at`, `status`, `final_letter_grade?`, `final_grade_points?`, `completed_at?`, `dropped_at?` | belongs_to Student, belongs_to Course |
| Grade       | `enrollment_id`, `assignment_name`, `grade_type`, `points_earned`, `points_possible`, `weight_percent`, `graded_at`, `comments?` | belongs_to Enrollment                       |

`?` marks nullable fields. Every model also carries auto-managed
`id`, `created_at`, `updated_at` (not editable).

## Filtering scenarios

* **Class roster for a course** — `Enrollment.course_id=X AND status='enrolled'` joined to `Student`, ordered by `Student.last_name`. The teacher's gradebook view.
* **Student transcript** — `Enrollment.student_id=X` ordered by `Course.academic_year DESC, term DESC`. Their full academic history.
* **Term-end report** — `Course.academic_year='2025-2026' AND term='spring' AND is_active=true`, count of `Enrollment` rows where `status='completed'` per course.
* **Grade-component drill-down** — `Grade.enrollment_id=Y` ordered by `graded_at`. Computes the student's running weighted total: `sum(points_earned/points_possible × weight_percent) / sum(weight_percent)`.
* **Capacity warning** — `Course.is_active=true AND enrolled_count >= capacity × 0.9`. Operator dashboard for over-subscribed courses.
* **Failing students mid-term** — `Grade.grade_type='midterm'` joined to `Enrollment` where `status='enrolled'`, computed grade < 60%. Drives advisor outreach.

## Conventions

`Enrollment.status`: `enrolled` / `dropped` / `completed`.
`Course.term`: `fall` / `winter` / `spring` / `summer`.
`Grade.grade_type`: `assignment` / `quiz` / `midterm` / `final` / `project`.
`Enrollment.final_letter_grade`: `A` / `B` / `C` / `D` / `F` / `I` (incomplete) / `W` (withdrew).

`Grade.weight_percent`: integer **0–100** representing this grade's
share of the course total. The application enforces the range AND
the per-course sum (typically `<= 100`); the schema stores the raw
value without enforcement. Same convention applies to any
`*_percent` field across all examples in this catalogue.

## Derived counters (staleness)

`Course.enrolled_count` is `editable: false` and reflects the count
of `Enrollment` rows where `status='enrolled'` for that course. The
application **must** keep this in sync — increment on enroll,
decrement on drop / complete. A nightly reconciliation job is a
common belt-and-braces pattern; consumers should treat the value as
"correct as of last write" rather than "real-time accurate".

## How to use

```
rustio new project school --schema schema.json
```

## Why this matters

Education systems run on cohorts that move through structured
terms. The Student-Course many-to-many through Enrollment, with
weighted grades layered on, is the same shape as cohort-based
training, certification programs, professional development, and
learning-management systems generally — pick the example that
fits your audience.

## Next

→ `examples/04-saas-core/` — multi-tenancy, members, projects, billing.

-- ───────────────────────────────────────────────────────────────
-- medflow — realistic seed data
--
-- Row counts:
--    8 departments
--   10 doctors
--   40 patients
--  120 appointments
--   60 prescriptions
--   40 invoices
--
-- Run once against a freshly-migrated database:
--
--     sqlite3 app.db < seed.sql
--
-- Idempotency: this file deliberately does NOT wipe existing rows.
-- If you run it twice you will hit UNIQUE-constraint failures on
-- department codes, doctor emails / license numbers, patient IDs /
-- emails, and invoice numbers. Rebuild the DB first instead.
-- ───────────────────────────────────────────────────────────────

PRAGMA foreign_keys = ON;

BEGIN TRANSACTION;

-- ═══════════════════════════════════════════════════════════════
-- Departments (8)
-- ═══════════════════════════════════════════════════════════════
INSERT INTO departments (id, name, code, is_active, head_doctor_id, created_at) VALUES
  (1, 'Cardiology',    'CARD', 1, NULL, datetime('now','-200 days')),
  (2, 'Pediatrics',    'PEDS', 1, NULL, datetime('now','-200 days')),
  (3, 'Neurology',     'NEUR', 1, NULL, datetime('now','-180 days')),
  (4, 'Oncology',      'ONCO', 1, NULL, datetime('now','-180 days')),
  (5, 'Orthopedics',   'ORTH', 1, NULL, datetime('now','-160 days')),
  (6, 'Emergency',     'EMER', 1, NULL, datetime('now','-160 days')),
  (7, 'Radiology',     'RADI', 1, NULL, datetime('now','-140 days')),
  (8, 'Pharmacy',      'PHAR', 1, NULL, datetime('now','-140 days'));

-- ═══════════════════════════════════════════════════════════════
-- Doctors (10) — each attached to a department
-- ═══════════════════════════════════════════════════════════════
INSERT INTO doctors (id, full_name, specialty, department_id, license_no, email, phone, years_experience, is_active, created_at) VALUES
  (1,  'Dr. Ahmed Abdelrahman', 'Interventional Cardiology', 1, 'LIC-00001', 'a.abdelrahman@medflow.local', '+20-100-1000001', 18, 1, datetime('now','-150 days')),
  (2,  'Dr. Hoda Saleh',        'General Pediatrics',        2, 'LIC-00002', 'h.saleh@medflow.local',         '+20-100-1000002', 12, 1, datetime('now','-145 days')),
  (3,  'Dr. Amr El-Sayed',      'Clinical Neurology',        3, 'LIC-00003', 'a.elsayed@medflow.local',       '+20-100-1000003', 22, 1, datetime('now','-140 days')),
  (4,  'Dr. Nadine Fahmy',      'Medical Oncology',          4, 'LIC-00004', 'n.fahmy@medflow.local',         '+20-100-1000004', 15, 1, datetime('now','-135 days')),
  (5,  'Dr. Mustafa Ali',       'Trauma Orthopedics',        5, 'LIC-00005', 'm.ali@medflow.local',           '+20-100-1000005', 20, 1, datetime('now','-130 days')),
  (6,  'Dr. Yasmin Naguib',     'Emergency Medicine',        6, 'LIC-00006', 'y.naguib@medflow.local',        '+20-100-1000006',  9, 1, datetime('now','-125 days')),
  (7,  'Dr. Mai Tarek',         'Diagnostic Radiology',      7, 'LIC-00007', 'm.tarek@medflow.local',         '+20-100-1000007', 11, 1, datetime('now','-120 days')),
  (8,  'Dr. Sherif Gamal',      'Preventive Cardiology',     1, 'LIC-00008', 's.gamal@medflow.local',         '+20-100-1000008',  7, 1, datetime('now','-110 days')),
  (9,  'Dr. Rania Adel',        'Pediatric Pulmonology',     2, 'LIC-00009', 'r.adel@medflow.local',          '+20-100-1000009', 14, 1, datetime('now','-100 days')),
  (10, 'Dr. Ismail Rouf',       'Orthopedic Surgery',        5, 'LIC-00010', 'i.rouf@medflow.local',          '+20-100-1000010', 25, 0, datetime('now','-90 days'));

-- Promote one doctor per department to head (only where we have one).
UPDATE departments SET head_doctor_id = 1  WHERE id = 1;
UPDATE departments SET head_doctor_id = 2  WHERE id = 2;
UPDATE departments SET head_doctor_id = 3  WHERE id = 3;
UPDATE departments SET head_doctor_id = 4  WHERE id = 4;
UPDATE departments SET head_doctor_id = 5  WHERE id = 5;
UPDATE departments SET head_doctor_id = 6  WHERE id = 6;
UPDATE departments SET head_doctor_id = 7  WHERE id = 7;
-- Pharmacy (8) has no attached doctor; head_doctor_id stays NULL.

-- ═══════════════════════════════════════════════════════════════
-- Patients (40) — varied DOB, gender, blood types, 2 inactive
-- ═══════════════════════════════════════════════════════════════
INSERT INTO patients (id, full_name, date_of_birth, gender, national_id, phone, email, blood_type, allergies, is_active, created_at) VALUES
  (1,  'Ahmed Hassan',      '1978-03-14 00:00:00', 'male',   'NID-00001', '+20-111-0000001', 'ahmed.hassan@mail.local',    'O+',  'Penicillin',            1, datetime('now','-300 days')),
  (2,  'Fatima Al-Zahra',   '1985-07-22 00:00:00', 'female', 'NID-00002', '+20-111-0000002', 'fatima.alzahra@mail.local',  'A+',  '',                      1, datetime('now','-290 days')),
  (3,  'Omar Mansour',      '1991-11-05 00:00:00', 'male',   'NID-00003', '+20-111-0000003', 'omar.mansour@mail.local',    'B+',  'Iodine contrast',       1, datetime('now','-280 days')),
  (4,  'Layla Ibrahim',     '1969-05-30 00:00:00', 'female', 'NID-00004', '+20-111-0000004', 'layla.ibrahim@mail.local',   'AB+', '',                      1, datetime('now','-270 days')),
  (5,  'Yusuf Khalil',      '2005-09-18 00:00:00', 'male',   'NID-00005', '+20-111-0000005', 'yusuf.khalil@mail.local',    'O-',  'Peanuts',               1, datetime('now','-260 days')),
  (6,  'Mariam Said',       '1956-01-12 00:00:00', 'female', 'NID-00006', '+20-111-0000006', 'mariam.said@mail.local',     'A-',  'Sulfa drugs, latex',    1, datetime('now','-250 days')),
  (7,  'Karim Farouk',      '1988-04-04 00:00:00', 'male',   'NID-00007', '+20-111-0000007', 'karim.farouk@mail.local',    'B-',  '',                      1, datetime('now','-240 days')),
  (8,  'Nour Hamdi',        '1994-10-27 00:00:00', 'female', 'NID-00008', '+20-111-0000008', 'nour.hamdi@mail.local',      'O+',  'Aspirin',               1, datetime('now','-230 days')),
  (9,  'Rami Nasser',       '1982-02-09 00:00:00', 'male',   'NID-00009', '+20-111-0000009', 'rami.nasser@mail.local',     'A+',  '',                      1, datetime('now','-220 days')),
  (10, 'Dina Tawfiq',       '1997-06-15 00:00:00', 'female', 'NID-00010', '+20-111-0000010', 'dina.tawfiq@mail.local',     'B+',  'Shellfish',             1, datetime('now','-210 days')),
  (11, 'Hassan Badawi',     '1942-08-21 00:00:00', 'male',   'NID-00011', '+20-111-0000011', 'hassan.badawi@mail.local',   'AB-', 'NSAIDs',                0, datetime('now','-205 days')),
  (12, 'Amira Sultan',      '1990-12-03 00:00:00', 'female', 'NID-00012', '+20-111-0000012', 'amira.sultan@mail.local',    'O+',  '',                      1, datetime('now','-200 days')),
  (13, 'Tarek Salama',      '1973-03-28 00:00:00', 'male',   'NID-00013', '+20-111-0000013', 'tarek.salama@mail.local',    'A+',  'Morphine',              1, datetime('now','-195 days')),
  (14, 'Noha Zaki',         '1986-07-19 00:00:00', 'female', 'NID-00014', '+20-111-0000014', 'noha.zaki@mail.local',       'B+',  '',                      1, datetime('now','-190 days')),
  (15, 'Walid Samir',       '2001-11-11 00:00:00', 'male',   'NID-00015', '+20-111-0000015', 'walid.samir@mail.local',     'O-',  'Latex',                 1, datetime('now','-185 days')),
  (16, 'Rana Abbas',        '1964-04-25 00:00:00', 'female', 'NID-00016', '+20-111-0000016', 'rana.abbas@mail.local',      'AB+', '',                      1, datetime('now','-180 days')),
  (17, 'Sami Fouad',        '1979-09-08 00:00:00', 'male',   'NID-00017', '+20-111-0000017', 'sami.fouad@mail.local',      'A-',  'Codeine',               0, datetime('now','-175 days')),
  (18, 'Hala Rashid',       '1993-01-14 00:00:00', 'female', 'NID-00018', '+20-111-0000018', 'hala.rashid@mail.local',     'B-',  '',                      1, datetime('now','-170 days')),
  (19, 'Ziad Maher',        '1987-05-22 00:00:00', 'male',   'NID-00019', '+20-111-0000019', 'ziad.maher@mail.local',      'O+',  'Peanuts, shellfish',    1, datetime('now','-165 days')),
  (20, 'Yara Lotfi',        '1999-10-06 00:00:00', 'female', 'NID-00020', '+20-111-0000020', 'yara.lotfi@mail.local',      'A+',  '',                      1, datetime('now','-160 days')),
  (21, 'Fadi Osman',        '1952-02-17 00:00:00', 'male',   'NID-00021', '+20-111-0000021', 'fadi.osman@mail.local',      'B+',  'Sulfa drugs',           1, datetime('now','-155 days')),
  (22, 'Iman Kamal',        '1976-06-30 00:00:00', 'female', 'NID-00022', '+20-111-0000022', 'iman.kamal@mail.local',      'AB+', '',                      1, datetime('now','-150 days')),
  (23, 'Adel Murad',        '2011-11-03 00:00:00', 'male',   'NID-00023', '+20-111-0000023', 'adel.murad@mail.local',      'O-',  'Penicillin',            1, datetime('now','-145 days')),
  (24, 'Lina Qasim',        '1981-03-19 00:00:00', 'female', 'NID-00024', '+20-111-0000024', 'lina.qasim@mail.local',      'A-',  '',                      1, datetime('now','-140 days')),
  (25, 'Basel Haddad',      '1995-07-25 00:00:00', 'male',   'NID-00025', '+20-111-0000025', 'basel.haddad@mail.local',    'B-',  'Bee stings',            1, datetime('now','-135 days')),
  (26, 'Reem Younis',       '1967-12-08 00:00:00', 'female', 'NID-00026', '+20-111-0000026', 'reem.younis@mail.local',     'O+',  '',                      1, datetime('now','-130 days')),
  (27, 'Majid Sabri',       '2008-04-02 00:00:00', 'male',   'NID-00027', '+20-111-0000027', 'majid.sabri@mail.local',     'A+',  'Dust mites',            1, datetime('now','-125 days')),
  (28, 'Sara Jaber',        '1984-08-16 00:00:00', 'female', 'NID-00028', '+20-111-0000028', 'sara.jaber@mail.local',      'B+',  '',                      1, datetime('now','-120 days')),
  (29, 'Khaled Rida',       '1946-01-29 00:00:00', 'male',   'NID-00029', '+20-111-0000029', 'khaled.rida@mail.local',     'AB-', 'NSAIDs, aspirin',       1, datetime('now','-115 days')),
  (30, 'Mona Darwish',      '1992-05-12 00:00:00', 'female', 'NID-00030', '+20-111-0000030', 'mona.darwish@mail.local',    'O-',  '',                      1, datetime('now','-110 days')),
  (31, 'Bilal Shawky',      '1971-10-24 00:00:00', 'male',   'NID-00031', '+20-111-0000031', 'bilal.shawky@mail.local',    'A+',  'Eggs',                  1, datetime('now','-105 days')),
  (32, 'Hana Diab',         '1989-02-07 00:00:00', 'female', 'NID-00032', '+20-111-0000032', 'hana.diab@mail.local',       'B-',  '',                      1, datetime('now','-100 days')),
  (33, 'Samir Helmy',       '2003-06-21 00:00:00', 'male',   'NID-00033', '+20-111-0000033', 'samir.helmy@mail.local',     'O+',  'Tree nuts',             1, datetime('now','-95 days')),
  (34, 'Nada Ezz',          '1958-11-04 00:00:00', 'female', 'NID-00034', '+20-111-0000034', 'nada.ezz@mail.local',        'AB+', '',                      1, datetime('now','-90 days')),
  (35, 'Gamal Wahba',       '1980-03-18 00:00:00', 'male',   'NID-00035', '+20-111-0000035', 'gamal.wahba@mail.local',     'A-',  'Gluten',                1, datetime('now','-85 days')),
  (36, 'Salma Rouhana',     '1996-07-31 00:00:00', 'female', 'NID-00036', '+20-111-0000036', 'salma.rouhana@mail.local',   'B+',  '',                      1, datetime('now','-80 days')),
  (37, 'Hisham Nabil',      '2013-12-13 00:00:00', 'male',   'NID-00037', '+20-111-0000037', 'hisham.nabil@mail.local',    'O-',  'Soy',                   1, datetime('now','-75 days')),
  (38, 'Rasha Fathi',       '1975-04-26 00:00:00', 'female', 'NID-00038', '+20-111-0000038', 'rasha.fathi@mail.local',     'A+',  '',                      1, datetime('now','-70 days')),
  (39, 'Nabil Afifi',       '1949-09-09 00:00:00', 'other',  'NID-00039', '+20-111-0000039', 'nabil.afifi@mail.local',     'AB-', 'Anaphylaxis to cefazolin', 1, datetime('now','-65 days')),
  (40, 'Farah Mikhail',     '1998-01-22 00:00:00', 'female', 'NID-00040', '+20-111-0000040', 'farah.mikhail@mail.local',   'O+',  '',                      1, datetime('now','-60 days'));

-- ═══════════════════════════════════════════════════════════════
-- Appointments (120) — spread across −30 to +30 days
-- ═══════════════════════════════════════════════════════════════
-- Batch 1: completed appointments in the past (60 rows)
INSERT INTO appointments (patient_id, doctor_id, scheduled_at, status, reason, notes, duration_minutes, priority, is_active, created_at) VALUES
  (1,  1, datetime('now','-28 days'), 'completed',   'Chest pain workup',           'ECG normal; stress test ordered.',              30, 7, 1, datetime('now','-35 days')),
  (2,  2, datetime('now','-27 days'), 'completed',   'Well-child visit',            'Vaccinations up to date.',                      30, 3, 1, datetime('now','-34 days')),
  (3,  3, datetime('now','-26 days'), 'completed',   'Migraine follow-up',          'Responded to sumatriptan.',                     20, 5, 1, datetime('now','-33 days')),
  (4,  4, datetime('now','-25 days'), 'completed',   'Oncology consult',            'Staging CT scheduled.',                         45, 9, 1, datetime('now','-32 days')),
  (5,  5, datetime('now','-25 days'), 'completed',   'Knee pain',                   'MRI pending.',                                  30, 4, 1, datetime('now','-31 days')),
  (6,  4, datetime('now','-24 days'), 'completed',   'Chemotherapy cycle 1',        'Tolerated well.',                              120, 9, 1, datetime('now','-31 days')),
  (7,  6, datetime('now','-24 days'), 'completed',   'ER — laceration',             'Sutures x5 to forearm.',                        30, 7, 1, datetime('now','-24 days')),
  (8,  8, datetime('now','-23 days'), 'completed',   'Preventive check',            'BP slightly elevated.',                         30, 3, 1, datetime('now','-30 days')),
  (9,  1, datetime('now','-22 days'), 'completed',   'Post-MI follow-up',           'Meds unchanged.',                               30, 8, 1, datetime('now','-29 days')),
  (10, 7, datetime('now','-22 days'), 'completed',   'Abdominal ultrasound',        'Normal.',                                       30, 4, 1, datetime('now','-28 days')),
  (11, 5, datetime('now','-21 days'), 'completed',   'Hip evaluation',              'Referred to physiotherapy.',                    30, 5, 1, datetime('now','-28 days')),
  (12, 2, datetime('now','-21 days'), 'completed',   'Asthma check-up',             'Peak flow improved.',                           30, 4, 1, datetime('now','-28 days')),
  (13, 3, datetime('now','-20 days'), 'completed',   'Seizure workup',              'EEG scheduled.',                                45, 8, 1, datetime('now','-27 days')),
  (14, 6, datetime('now','-20 days'), 'completed',   'ER — chest pain',             'Cardiac enzymes negative.',                     60, 8, 1, datetime('now','-20 days')),
  (15, 9, datetime('now','-19 days'), 'completed',   'Cough, 3 weeks',              'Pulmonary function normal.',                    30, 4, 1, datetime('now','-25 days')),
  (16, 4, datetime('now','-19 days'), 'completed',   'Chemotherapy cycle 2',        'Mild nausea.',                                 120, 9, 1, datetime('now','-26 days')),
  (17, 8, datetime('now','-18 days'), 'completed',   'Cholesterol follow-up',       'Lipid panel improved.',                         30, 4, 1, datetime('now','-25 days')),
  (18, 2, datetime('now','-18 days'), 'completed',   'Rash evaluation',             'Topical steroid prescribed.',                   20, 3, 1, datetime('now','-24 days')),
  (19, 3, datetime('now','-17 days'), 'completed',   'Parkinsonism assessment',     'Dose adjusted.',                                45, 7, 1, datetime('now','-24 days')),
  (20, 5, datetime('now','-17 days'), 'completed',   'Back pain follow-up',         'Responded to physio.',                          30, 4, 1, datetime('now','-23 days')),
  (21, 1, datetime('now','-16 days'), 'completed',   'Arrhythmia check',            'Holter attached.',                              30, 7, 1, datetime('now','-22 days')),
  (22, 7, datetime('now','-16 days'), 'completed',   'CT abdomen',                  'No acute findings.',                            45, 5, 1, datetime('now','-22 days')),
  (23, 2, datetime('now','-15 days'), 'completed',   'Ear infection',               'Amoxicillin prescribed.',                       20, 3, 1, datetime('now','-21 days')),
  (24, 4, datetime('now','-15 days'), 'completed',   'Chemotherapy cycle 3',        'Dose reduced due to cytopenia.',               120, 9, 1, datetime('now','-22 days')),
  (25, 6, datetime('now','-14 days'), 'completed',   'ER — fracture',               'Closed reduction, cast applied.',               90, 8, 1, datetime('now','-14 days')),
  (26, 8, datetime('now','-14 days'), 'completed',   'Annual cardio screen',        'Normal.',                                       30, 2, 1, datetime('now','-21 days')),
  (27, 9, datetime('now','-13 days'), 'completed',   'Pediatric pneumonia fup',     'Clear on auscultation.',                        30, 5, 1, datetime('now','-20 days')),
  (28, 3, datetime('now','-13 days'), 'completed',   'Tension headache',            'Sleep hygiene advised.',                        20, 3, 1, datetime('now','-19 days')),
  (29, 5, datetime('now','-12 days'), 'completed',   'Shoulder MRI follow-up',      'Rotator cuff tear partial.',                    30, 6, 1, datetime('now','-19 days')),
  (30, 1, datetime('now','-12 days'), 'completed',   'Hypertension follow-up',      'Started losartan 50.',                          30, 6, 1, datetime('now','-18 days')),
  (31, 6, datetime('now','-11 days'), 'completed',   'ER — allergic reaction',      'Epi administered, observed 4h.',                90, 9, 1, datetime('now','-11 days')),
  (32, 2, datetime('now','-11 days'), 'completed',   'School vaccination',          'MMR booster.',                                  20, 2, 1, datetime('now','-18 days')),
  (33, 4, datetime('now','-10 days'), 'completed',   'Oncology consult',            'Second opinion.',                               45, 7, 1, datetime('now','-17 days')),
  (34, 7, datetime('now','-10 days'), 'completed',   'Chest X-ray',                 'No acute findings.',                            15, 3, 1, datetime('now','-17 days')),
  (35, 3, datetime('now','-9 days'),  'completed',   'Dementia evaluation',         'MMSE 24/30.',                                   60, 6, 1, datetime('now','-16 days')),
  (36, 9, datetime('now','-9 days'),  'completed',   'Bronchiolitis follow-up',     'Saturating 98% on air.',                        30, 5, 1, datetime('now','-16 days')),
  (37, 5, datetime('now','-8 days'),  'completed',   'Post-op knee follow-up',      'Wound healing well.',                           30, 5, 1, datetime('now','-45 days')),
  (38, 8, datetime('now','-8 days'),  'completed',   'Preventive — smoker',         'Counselling on cessation.',                     45, 5, 1, datetime('now','-15 days')),
  (39, 1, datetime('now','-7 days'),  'completed',   'Palpitations',                'Event monitor requested.',                      30, 6, 1, datetime('now','-14 days')),
  (40, 4, datetime('now','-7 days'),  'completed',   'Oncology follow-up',          'Stable.',                                       30, 7, 1, datetime('now','-14 days')),
  (1,  1, datetime('now','-6 days'),  'completed',   'Stress test',                 'Mildly positive at 85%.',                       45, 7, 1, datetime('now','-13 days')),
  (6,  4, datetime('now','-6 days'),  'completed',   'Chemotherapy cycle 4',        'WBC recovering.',                              120, 9, 1, datetime('now','-13 days')),
  (11, 5, datetime('now','-5 days'),  'completed',   'Physio review',               'Range of motion improving.',                    30, 4, 1, datetime('now','-12 days')),
  (13, 3, datetime('now','-5 days'),  'completed',   'EEG results review',          'Focal spikes left temporal.',                   30, 8, 1, datetime('now','-12 days')),
  (16, 4, datetime('now','-4 days'),  'completed',   'Chemotherapy cycle 5',        'Febrile neutropenia risk.',                    120, 10, 1, datetime('now','-11 days')),
  (19, 3, datetime('now','-4 days'),  'completed',   'Parkinson follow-up',         'On/off fluctuations.',                          45, 7, 1, datetime('now','-11 days')),
  (21, 1, datetime('now','-3 days'),  'completed',   'Holter results',              'Sinus rhythm, rare PACs.',                      20, 4, 1, datetime('now','-10 days')),
  (25, 5, datetime('now','-3 days'),  'completed',   'Fracture follow-up',          'X-ray: healing.',                               20, 4, 1, datetime('now','-10 days')),
  (27, 9, datetime('now','-2 days'),  'completed',   'Peds check',                  'All clear.',                                    20, 2, 1, datetime('now','-9 days')),
  (29, 5, datetime('now','-2 days'),  'completed',   'Shoulder injection',          'Subacromial steroid.',                          30, 5, 1, datetime('now','-9 days')),
  (33, 4, datetime('now','-1 days'),  'completed',   'Oncology MDT',                'Team discussed case.',                          60, 7, 1, datetime('now','-8 days')),
  (37, 5, datetime('now','-1 days'),  'completed',   'Brace fitting',               'Patient comfortable.',                          30, 4, 1, datetime('now','-8 days')),
  (2,  9, datetime('now','-1 days'),  'completed',   'Pediatric asthma',            'Reviewed action plan.',                         30, 5, 1, datetime('now','-8 days')),
  (22, 7, datetime('now','-1 days'),  'completed',   'MRI brain',                   'No intracranial pathology.',                    60, 6, 1, datetime('now','-7 days')),
  (30, 1, datetime('now','-1 days'),  'completed',   'BP recheck',                  'Controlled 124/78.',                            20, 4, 1, datetime('now','-7 days')),
  (24, 4, datetime('now','-1 days'),  'completed',   'Chemotherapy cycle 6',        'Final cycle.',                                 120, 10, 1, datetime('now','-7 days')),
  (18, 2, datetime('now','-1 days'),  'completed',   'Eczema review',               'Improving, taper topical.',                     20, 2, 1, datetime('now','-7 days')),
  (14, 6, datetime('now','-1 days'),  'completed',   'ER — dizziness',              'Benign positional vertigo.',                    60, 6, 1, datetime('now','-1 days')),
  (5,  9, datetime('now','-1 days'),  'completed',   'Allergy workup',              'Skin test positive to peanut.',                 45, 5, 1, datetime('now','-6 days')),
  (40, 8, datetime('now','-1 days'),  'completed',   'New patient intake',          'History taken.',                                45, 3, 1, datetime('now','-6 days'));

-- Batch 2: scheduled appointments today and the next 30 days (40 rows)
INSERT INTO appointments (patient_id, doctor_id, scheduled_at, status, reason, notes, duration_minutes, priority, is_active, created_at) VALUES
  (1,  1, datetime('now','+1 days'),  'scheduled', 'Cardio review',             '',                                              30, 6, 1, datetime('now','-5 days')),
  (3,  3, datetime('now','+1 days'),  'scheduled', 'Migraine review',           '',                                              30, 5, 1, datetime('now','-5 days')),
  (4,  4, datetime('now','+2 days'),  'scheduled', 'Onco follow-up',            '',                                              30, 7, 1, datetime('now','-5 days')),
  (5,  9, datetime('now','+2 days'),  'scheduled', 'Pediatric allergy review',  '',                                              30, 4, 1, datetime('now','-5 days')),
  (7,  6, datetime('now','+3 days'),  'scheduled', 'Wound check',               '',                                              20, 5, 1, datetime('now','-4 days')),
  (8,  1, datetime('now','+3 days'),  'scheduled', 'Arrhythmia follow-up',      '',                                              30, 6, 1, datetime('now','-4 days')),
  (9,  8, datetime('now','+4 days'),  'scheduled', 'Cholesterol recheck',       '',                                              30, 4, 1, datetime('now','-4 days')),
  (10, 7, datetime('now','+4 days'),  'scheduled', 'Repeat ultrasound',         '',                                              30, 4, 1, datetime('now','-4 days')),
  (12, 2, datetime('now','+5 days'),  'scheduled', 'Asthma review',             '',                                              30, 4, 1, datetime('now','-3 days')),
  (13, 3, datetime('now','+5 days'),  'scheduled', 'Epilepsy review',           '',                                              45, 8, 1, datetime('now','-3 days')),
  (14, 8, datetime('now','+6 days'),  'scheduled', 'BP review',                 '',                                              30, 5, 1, datetime('now','-3 days')),
  (15, 9, datetime('now','+6 days'),  'scheduled', 'Teen check',                '',                                              20, 2, 1, datetime('now','-3 days')),
  (16, 4, datetime('now','+7 days'),  'scheduled', 'Onco infusion',             '',                                             120, 9, 1, datetime('now','-2 days')),
  (17, 5, datetime('now','+7 days'),  'scheduled', 'Back pain re-eval',         '',                                              30, 5, 1, datetime('now','-2 days')),
  (18, 2, datetime('now','+8 days'),  'scheduled', 'Follow-up rash',            '',                                              20, 2, 1, datetime('now','-2 days')),
  (19, 3, datetime('now','+8 days'),  'scheduled', 'Parkinson review',          '',                                              30, 7, 1, datetime('now','-2 days')),
  (20, 6, datetime('now','+9 days'),  'scheduled', 'ER follow-up',              '',                                              30, 4, 1, datetime('now','-1 days')),
  (21, 1, datetime('now','+9 days'),  'scheduled', 'Event monitor review',      '',                                              30, 5, 1, datetime('now','-1 days')),
  (22, 7, datetime('now','+10 days'), 'scheduled', 'Repeat CT',                 '',                                              45, 5, 1, datetime('now','-1 days')),
  (23, 2, datetime('now','+10 days'), 'scheduled', 'Pediatric check',           '',                                              20, 2, 1, datetime('now','-1 days')),
  (24, 4, datetime('now','+11 days'), 'scheduled', 'Onco follow-up',            '',                                              30, 7, 1, datetime('now','-1 days')),
  (25, 5, datetime('now','+11 days'), 'scheduled', 'Cast removal',              '',                                              20, 4, 1, datetime('now','-1 days')),
  (26, 8, datetime('now','+12 days'), 'scheduled', 'Annual screen',             '',                                              30, 2, 1, datetime('now')),
  (27, 9, datetime('now','+12 days'), 'scheduled', 'Pneumonia 2-week review',   '',                                              30, 3, 1, datetime('now')),
  (28, 3, datetime('now','+13 days'), 'scheduled', 'Headache follow-up',        '',                                              20, 3, 1, datetime('now')),
  (29, 5, datetime('now','+13 days'), 'scheduled', 'Shoulder re-eval',          '',                                              30, 4, 1, datetime('now')),
  (30, 1, datetime('now','+14 days'), 'scheduled', 'BP recheck',                '',                                              20, 4, 1, datetime('now')),
  (31, 2, datetime('now','+14 days'), 'scheduled', 'Allergy follow-up',         '',                                              30, 4, 1, datetime('now')),
  (32, 3, datetime('now','+15 days'), 'scheduled', 'New neuro patient',         '',                                              60, 6, 1, datetime('now')),
  (33, 4, datetime('now','+15 days'), 'scheduled', 'Onco infusion',             '',                                             120, 9, 1, datetime('now')),
  (34, 5, datetime('now','+17 days'), 'scheduled', 'Hip assessment',            '',                                              45, 6, 1, datetime('now')),
  (35, 6, datetime('now','+17 days'), 'scheduled', 'Follow-up ER admission',    '',                                              30, 5, 1, datetime('now')),
  (36, 9, datetime('now','+18 days'), 'scheduled', 'Asthma recheck',            '',                                              30, 4, 1, datetime('now')),
  (37, 2, datetime('now','+18 days'), 'scheduled', 'Peds new patient',          '',                                              45, 3, 1, datetime('now')),
  (38, 1, datetime('now','+20 days'), 'scheduled', 'Cardio review',             '',                                              30, 5, 1, datetime('now')),
  (39, 4, datetime('now','+21 days'), 'scheduled', 'Palliative consult',        '',                                              60, 9, 1, datetime('now')),
  (40, 8, datetime('now','+22 days'), 'scheduled', 'Cardio screen',             '',                                              30, 3, 1, datetime('now')),
  (2,  2, datetime('now','+25 days'), 'scheduled', 'Pediatric annual',          '',                                              30, 2, 1, datetime('now')),
  (11, 5, datetime('now','+27 days'), 'scheduled', 'Orthopedic follow-up',      '',                                              30, 4, 1, datetime('now')),
  (6,  4, datetime('now','+30 days'), 'scheduled', 'Onco review',               '',                                              30, 7, 1, datetime('now'));

-- Batch 3: other statuses — cancelled, no_show, checked_in, in_progress (20 rows)
INSERT INTO appointments (patient_id, doctor_id, scheduled_at, status, reason, notes, duration_minutes, priority, is_active, created_at) VALUES
  (7,  5, datetime('now','-8 days'),  'cancelled',   'Ortho consult',           'Patient rescheduled via phone.',                30, 4, 1, datetime('now','-15 days')),
  (10, 3, datetime('now','-6 days'),  'cancelled',   'Neuro review',            'Doctor on leave.',                              30, 5, 1, datetime('now','-12 days')),
  (13, 8, datetime('now','-5 days'),  'cancelled',   'Cardio screen',           'Insurance issue.',                              30, 3, 1, datetime('now','-11 days')),
  (15, 2, datetime('now','-4 days'),  'cancelled',   'Peds routine',            'Family travel.',                                20, 2, 1, datetime('now','-10 days')),
  (19, 6, datetime('now','-3 days'),  'cancelled',   'ER follow-up',            'Condition resolved.',                           30, 3, 1, datetime('now','-9 days')),
  (23, 9, datetime('now','-7 days'),  'no_show',     'Peds routine',            'Did not attend, phone unreachable.',            30, 2, 1, datetime('now','-14 days')),
  (26, 1, datetime('now','-5 days'),  'no_show',     'Cardio check',            'No response to reminders.',                     30, 5, 1, datetime('now','-12 days')),
  (28, 3, datetime('now','-4 days'),  'no_show',     'Headache workup',         'Patient rescheduled later.',                    30, 4, 1, datetime('now','-10 days')),
  (31, 7, datetime('now','-3 days'),  'no_show',     'Imaging follow-up',       'Did not attend.',                               45, 5, 1, datetime('now','-9 days')),
  (34, 5, datetime('now'),            'checked_in',  'Hip injection',           'Checked in at reception.',                      30, 6, 1, datetime('now','-3 days')),
  (35, 2, datetime('now'),            'checked_in',  'Peds vaccination',        'In waiting room.',                              20, 3, 1, datetime('now','-3 days')),
  (36, 4, datetime('now'),            'in_progress', 'Onco consult',            'Patient with doctor.',                          45, 8, 1, datetime('now','-2 days')),
  (37, 6, datetime('now'),            'in_progress', 'ER — high fever',         'Bloods taken.',                                 60, 8, 1, datetime('now')),
  (38, 1, datetime('now'),            'checked_in',  'Cardio review',           '',                                              30, 5, 1, datetime('now','-2 days')),
  (39, 4, datetime('now'),            'in_progress', 'Palliative review',       'With family.',                                  60, 9, 1, datetime('now','-1 days')),
  (22, 3, datetime('now','-2 days'),  'cancelled',   'Neuro review',            'Patient unwell, flu.',                          30, 4, 1, datetime('now','-9 days')),
  (8,  6, datetime('now','-1 days'),  'no_show',     'ER follow-up',            '',                                              30, 5, 1, datetime('now','-8 days')),
  (32, 4, datetime('now','-10 days'), 'cancelled',   'Onco consult',            'Lab results pending.',                          30, 6, 1, datetime('now','-17 days')),
  (17, 1, datetime('now','-9 days'),  'cancelled',   'Post-MI follow-up',       'Patient deceased (inactive).',                  30, 4, 0, datetime('now','-16 days')),
  (11, 8, datetime('now','-4 days'),  'no_show',     'Preventive cardio',       'Inactive patient (deceased).',                  30, 3, 0, datetime('now','-11 days'));

-- ═══════════════════════════════════════════════════════════════
-- Prescriptions (60) — attached to completed appointments 1..60
-- ═══════════════════════════════════════════════════════════════
-- Attach one prescription to each of the first 60 completed appointments.
-- These INSERT rows use literal appointment_ids 1..60 because that's
-- the order the completed batch above was inserted.
INSERT INTO prescriptions (appointment_id, patient_id, doctor_id, medication, dosage, frequency, duration_days, is_refillable, refills_remaining, notes, created_at) VALUES
  (1,  1,  1, 'Aspirin 81 mg',           '1 tab',          'daily',       90, 1, 3, 'Cardioprotective.',             datetime('now','-28 days')),
  (2,  2,  2, 'Amoxicillin 250 mg/5 ml', '5 ml',           'tid',          7, 0, 0, 'Complete course.',              datetime('now','-27 days')),
  (3,  3,  3, 'Sumatriptan 50 mg',       '1 tab prn',      'at onset',    30, 1, 2, 'Max 2/day.',                    datetime('now','-26 days')),
  (4,  4,  4, 'Ondansetron 8 mg',        '1 tab',          'tid prn',     14, 1, 1, 'For CINV.',                     datetime('now','-25 days')),
  (5,  5,  5, 'Ibuprofen 400 mg',        '1 tab',          'tid with food',7, 0, 0, 'Take after meals.',             datetime('now','-25 days')),
  (6,  6,  4, 'Dexamethasone 4 mg',      '1 tab',          'bid',          3, 0, 0, 'Anti-emetic prophylaxis.',      datetime('now','-24 days')),
  (7,  7,  6, 'Tetanus booster',         '0.5 ml IM',      'single dose',  1, 0, 0, 'Given in ER.',                  datetime('now','-24 days')),
  (8,  8,  8, 'Losartan 50 mg',          '1 tab',          'daily',       30, 1, 3, 'BP control.',                   datetime('now','-23 days')),
  (9,  9,  1, 'Atorvastatin 40 mg',      '1 tab',          'daily',       90, 1, 3, 'Post-MI secondary prevention.', datetime('now','-22 days')),
  (10, 10, 7, 'Paracetamol 1 g',         '1 tab',          'qid prn',      7, 0, 0, 'Pain relief post-scan.',        datetime('now','-22 days')),
  (11, 11, 5, 'Diclofenac gel 1%',       'topical',        'tid',         14, 1, 2, 'Apply to affected joint.',      datetime('now','-21 days')),
  (12, 12, 2, 'Albuterol inhaler',       '2 puffs',        'qid prn',     30, 1, 3, 'Rescue inhaler.',               datetime('now','-21 days')),
  (13, 13, 3, 'Levetiracetam 500 mg',    '1 tab',          'bid',         30, 1, 3, 'Start low, titrate.',           datetime('now','-20 days')),
  (14, 14, 6, 'Aspirin 300 mg',          '1 tab',          'daily',       30, 1, 2, 'Cardioprotective.',             datetime('now','-20 days')),
  (15, 15, 9, 'Montelukast 10 mg',       '1 tab',          'nightly',     30, 1, 2, 'Asthma control.',               datetime('now','-19 days')),
  (16, 16, 4, 'Cisplatin',               '75 mg/m2 IV',    'q3 weeks',     1, 0, 0, 'Cycle 2 — admin in day unit.',  datetime('now','-19 days')),
  (17, 17, 8, 'Atorvastatin 20 mg',      '1 tab',          'daily',       90, 1, 3, 'Cholesterol.',                  datetime('now','-18 days')),
  (18, 18, 2, 'Hydrocortisone 1% cream', 'topical',        'bid',         14, 1, 1, 'Apply to rash.',                datetime('now','-18 days')),
  (19, 19, 3, 'Levodopa/carbidopa',      '1 tab',          'tid',         30, 1, 3, 'Continue current dose.',        datetime('now','-17 days')),
  (20, 20, 5, 'Naproxen 500 mg',         '1 tab',          'bid with food',14, 0, 0, 'For acute back pain.',         datetime('now','-17 days')),
  (21, 21, 1, 'Metoprolol 50 mg',        '1 tab',          'bid',         30, 1, 2, 'Rate control.',                 datetime('now','-16 days')),
  (22, 22, 7, 'Omeprazole 20 mg',        '1 tab',          'daily',       30, 1, 2, 'Reflux control.',               datetime('now','-16 days')),
  (23, 23, 2, 'Amoxicillin 500 mg',      '1 tab',          'tid',          7, 0, 0, 'Otitis media.',                 datetime('now','-15 days')),
  (24, 24, 4, 'Filgrastim 300 mcg',      '1 SC inj',       'daily x5',     5, 0, 0, 'For neutropenia support.',      datetime('now','-15 days')),
  (25, 25, 6, 'Tramadol 50 mg',          '1 tab',          'qid prn',      5, 0, 0, 'Short course post-fracture.',   datetime('now','-14 days')),
  (26, 26, 8, 'Rosuvastatin 10 mg',      '1 tab',          'daily',       90, 1, 3, 'Primary prevention.',           datetime('now','-14 days')),
  (27, 27, 9, 'Azithromycin 200 mg/5 ml','5 ml',           'daily',        5, 0, 0, 'Pediatric course.',             datetime('now','-13 days')),
  (28, 28, 3, 'Paracetamol 1 g',         '1 tab',          'qid prn',      7, 0, 0, 'Tension headache.',             datetime('now','-13 days')),
  (29, 29, 5, 'Meloxicam 15 mg',         '1 tab',          'daily',       14, 1, 1, 'Partial cuff tear.',            datetime('now','-12 days')),
  (30, 30, 1, 'Losartan 50 mg',          '1 tab',          'daily',       30, 1, 3, 'New diagnosis HTN.',            datetime('now','-12 days')),
  (31, 31, 6, 'Epinephrine auto-inj',    '0.3 mg IM',      'prn',         90, 1, 1, 'Carry at all times.',           datetime('now','-11 days')),
  (32, 32, 2, 'MMR vaccine',             '0.5 ml SC',      'single dose',  1, 0, 0, 'School entry booster.',         datetime('now','-11 days')),
  (33, 33, 4, 'Dexamethasone 4 mg',      '1 tab',          'bid',          3, 0, 0, 'Premedication.',                datetime('now','-10 days')),
  (34, 34, 7, 'Contrast agent iohexol',  'IV bolus',       'single dose',  1, 0, 0, 'For CT.',                       datetime('now','-10 days')),
  (35, 35, 3, 'Donepezil 5 mg',          '1 tab',          'nightly',     30, 1, 2, 'Start low.',                    datetime('now','-9 days')),
  (36, 36, 9, 'Salbutamol nebules',      '2.5 mg',         'qid prn',     14, 1, 1, 'Pediatric dose.',               datetime('now','-9 days')),
  (37, 37, 5, 'Celecoxib 200 mg',        '1 cap',          'bid',         30, 1, 2, 'Post-op analgesia.',            datetime('now','-8 days')),
  (38, 38, 8, 'Varenicline 1 mg',        '1 tab',          'bid',         84, 1, 2, 'Smoking cessation.',            datetime('now','-8 days')),
  (39, 39, 1, 'Bisoprolol 2.5 mg',       '1 tab',          'daily',       30, 1, 2, 'Symptomatic palpitations.',     datetime('now','-7 days')),
  (40, 40, 4, 'Letrozole 2.5 mg',        '1 tab',          'daily',       90, 1, 5, 'Continue.',                     datetime('now','-7 days')),
  (41, 1,  1, 'Atorvastatin 80 mg',      '1 tab',          'daily',       90, 1, 3, 'Post stress test.',             datetime('now','-6 days')),
  (42, 6,  4, 'Ondansetron 8 mg',        '1 tab',          'tid prn',      7, 1, 0, 'For nausea.',                   datetime('now','-6 days')),
  (43, 11, 5, 'Paracetamol 1 g',         '1 tab',          'qid prn',      7, 0, 0, 'Post-physio.',                  datetime('now','-5 days')),
  (44, 13, 3, 'Carbamazepine 200 mg',    '1 tab',          'bid',         30, 1, 2, 'Adjusted for EEG findings.',    datetime('now','-5 days')),
  (45, 16, 4, 'Pegfilgrastim 6 mg',      '1 SC inj',       'single dose',  1, 0, 0, 'Prophylactic.',                 datetime('now','-4 days')),
  (46, 19, 3, 'Entacapone 200 mg',       '1 tab',          'with each levodopa dose', 30, 1, 2, 'For fluctuations.', datetime('now','-4 days')),
  (47, 21, 1, 'Bisoprolol 5 mg',         '1 tab',          'daily',       30, 1, 2, 'Holter findings.',              datetime('now','-3 days')),
  (48, 25, 5, 'Ibuprofen 400 mg',        '1 tab',          'tid with food',7, 0, 0, 'Mild pain.',                    datetime('now','-3 days')),
  (49, 27, 9, 'Fluticasone inhaler',     '2 puffs',        'bid',         30, 1, 2, 'Prevention.',                   datetime('now','-2 days')),
  (50, 29, 5, 'Triamcinolone 40 mg',     '1 ml injection', 'single dose',  1, 0, 0, 'Subacromial injection.',        datetime('now','-2 days')),
  (51, 33, 4, 'Zoledronic acid 4 mg',    '1 IV infusion',  'single dose',  1, 0, 0, 'For bone protection.',          datetime('now','-1 days')),
  (52, 37, 5, 'Diclofenac 50 mg',        '1 tab',          'tid',         14, 1, 1, 'Pain control.',                 datetime('now','-1 days')),
  (53, 2,  9, 'Salmeterol/fluticasone',  '1 inh',          'bid',         30, 1, 2, 'Asthma maintenance.',           datetime('now','-1 days')),
  (54, 22, 7, 'Gadolinium contrast',     'IV',             'single dose',  1, 0, 0, 'Used for MRI brain.',           datetime('now','-1 days')),
  (55, 30, 1, 'Amlodipine 5 mg',         '1 tab',          'daily',       30, 1, 3, 'BP combo therapy.',             datetime('now','-1 days')),
  (56, 24, 4, 'Neulasta 6 mg',           '1 SC inj',       'single dose',  1, 0, 0, 'Final cycle support.',          datetime('now','-1 days')),
  (57, 18, 2, 'Tacrolimus ointment',     'topical',        'bid',         14, 1, 1, 'Taper eczema therapy.',         datetime('now','-1 days')),
  (58, 14, 6, 'Prochlorperazine 5 mg',   '1 tab',          'tid prn',      5, 0, 0, 'Vertigo.',                      datetime('now','-1 days')),
  (59, 5,  9, 'EpiPen',                  '0.3 mg IM',      'prn',         90, 1, 1, 'Newly identified peanut allergy.', datetime('now','-1 days')),
  (60, 40, 8, 'Aspirin 81 mg',           '1 tab',          'daily',       90, 1, 3, 'Baseline cardioprotection.',    datetime('now','-1 days'));

-- ═══════════════════════════════════════════════════════════════
-- Invoices (40) — mixed statuses, mixed currencies
-- ═══════════════════════════════════════════════════════════════
-- 30 tied to a completed appointment; 10 are standalone (appointment_id NULL)
INSERT INTO invoices (invoice_number, patient_id, appointment_id, amount_cents, currency, status, issued_at, paid_at, notes, created_at) VALUES
  ('INV-2026-0001',  1, 1,   45000,  'USD', 'paid',     datetime('now','-28 days'), datetime('now','-21 days'), 'Consult + ECG.',              datetime('now','-28 days')),
  ('INV-2026-0002',  2, 2,   18000,  'USD', 'paid',     datetime('now','-27 days'), datetime('now','-20 days'), 'Well-child visit.',           datetime('now','-27 days')),
  ('INV-2026-0003',  3, 3,   22000,  'USD', 'paid',     datetime('now','-26 days'), datetime('now','-19 days'), 'Neurology consult.',          datetime('now','-26 days')),
  ('INV-2026-0004',  4, 4,  125000,  'USD', 'paid',     datetime('now','-25 days'), datetime('now','-18 days'), 'Oncology consult.',           datetime('now','-25 days')),
  ('INV-2026-0005',  5, 5,   28000,  'USD', 'paid',     datetime('now','-25 days'), datetime('now','-18 days'), 'Orthopedic workup.',          datetime('now','-25 days')),
  ('INV-2026-0006',  6, 6,  320000,  'USD', 'paid',     datetime('now','-24 days'), datetime('now','-10 days'), 'Chemotherapy cycle 1.',       datetime('now','-24 days')),
  ('INV-2026-0007',  7, 7,   65000,  'USD', 'paid',     datetime('now','-24 days'), datetime('now','-23 days'), 'ER laceration + sutures.',    datetime('now','-24 days')),
  ('INV-2026-0008',  8, 8,   24000,  'USD', 'paid',     datetime('now','-23 days'), datetime('now','-16 days'), 'Preventive visit.',           datetime('now','-23 days')),
  ('INV-2026-0009',  9, 9,   30000,  'USD', 'paid',     datetime('now','-22 days'), datetime('now','-15 days'), 'Post-MI follow-up.',          datetime('now','-22 days')),
  ('INV-2026-0010', 10, 10,  42000,  'EUR', 'paid',     datetime('now','-22 days'), datetime('now','-15 days'), 'US scan.',                    datetime('now','-22 days')),
  ('INV-2026-0011', 11, 11,  28000,  'USD', 'paid',     datetime('now','-21 days'), datetime('now','-14 days'), 'Ortho + physio referral.',    datetime('now','-21 days')),
  ('INV-2026-0012', 12, 12,  22000,  'USD', 'paid',     datetime('now','-21 days'), datetime('now','-14 days'), 'Peds asthma review.',         datetime('now','-21 days')),
  ('INV-2026-0013', 13, 13,  78000,  'USD', 'paid',     datetime('now','-20 days'), datetime('now','-13 days'), 'EEG scheduled + consult.',    datetime('now','-20 days')),
  ('INV-2026-0014', 14, 14,  95000,  'USD', 'paid',     datetime('now','-20 days'), datetime('now','-13 days'), 'ER chest pain workup.',       datetime('now','-20 days')),
  ('INV-2026-0015', 15, 15,  30000,  'SAR', 'paid',     datetime('now','-19 days'), datetime('now','-12 days'), 'Pulmonary function.',         datetime('now','-19 days')),
  ('INV-2026-0016', 16, 16, 320000,  'USD', 'paid',     datetime('now','-19 days'), datetime('now','-5 days'),  'Chemotherapy cycle 2.',       datetime('now','-19 days')),
  ('INV-2026-0017', 17, 17,  26000,  'USD', 'overdue',  datetime('now','-18 days'), NULL,                        'Cholesterol follow-up.',      datetime('now','-18 days')),
  ('INV-2026-0018', 18, 18,  18000,  'USD', 'paid',     datetime('now','-18 days'), datetime('now','-11 days'), 'Rash evaluation.',            datetime('now','-18 days')),
  ('INV-2026-0019', 19, 19,  75000,  'USD', 'paid',     datetime('now','-17 days'), datetime('now','-10 days'), 'Parkinsonism review.',        datetime('now','-17 days')),
  ('INV-2026-0020', 20, 20,  28000,  'USD', 'overdue',  datetime('now','-17 days'), NULL,                        'Back pain follow-up.',        datetime('now','-17 days')),
  ('INV-2026-0021', 21, 21,  35000,  'AED', 'paid',     datetime('now','-16 days'), datetime('now','-9 days'),  'Cardio arrhythmia check.',    datetime('now','-16 days')),
  ('INV-2026-0022', 22, 22,  68000,  'USD', 'paid',     datetime('now','-16 days'), datetime('now','-9 days'),  'CT abdomen.',                 datetime('now','-16 days')),
  ('INV-2026-0023', 23, 23,  18000,  'USD', 'paid',     datetime('now','-15 days'), datetime('now','-8 days'),  'Otitis media treatment.',     datetime('now','-15 days')),
  ('INV-2026-0024', 24, 24, 320000,  'USD', 'issued',   datetime('now','-15 days'), NULL,                        'Chemotherapy cycle 3.',       datetime('now','-15 days')),
  ('INV-2026-0025', 25, 25, 120000,  'USD', 'paid',     datetime('now','-14 days'), datetime('now','-7 days'),  'ER fracture reduction.',      datetime('now','-14 days')),
  ('INV-2026-0026', 26, 26,  24000,  'USD', 'issued',   datetime('now','-14 days'), NULL,                        'Annual cardio screen.',       datetime('now','-14 days')),
  ('INV-2026-0027', 27, 27,  22000,  'USD', 'paid',     datetime('now','-13 days'), datetime('now','-6 days'),  'Peds pneumonia review.',      datetime('now','-13 days')),
  ('INV-2026-0028', 28, 28,  16000,  'USD', 'paid',     datetime('now','-13 days'), datetime('now','-6 days'),  'Tension headache.',           datetime('now','-13 days')),
  ('INV-2026-0029', 29, 29,  28000,  'USD', 'paid',     datetime('now','-12 days'), datetime('now','-5 days'),  'Shoulder MRI follow-up.',     datetime('now','-12 days')),
  ('INV-2026-0030', 30, 30,  25000,  'USD', 'paid',     datetime('now','-12 days'), datetime('now','-5 days'),  'HTN follow-up + meds.',       datetime('now','-12 days')),
  -- Standalone invoices (no appointment)
  ('INV-2026-0031',  1, NULL,  8000, 'USD', 'paid',     datetime('now','-10 days'), datetime('now','-3 days'),  'Repeat prescription fee.',    datetime('now','-10 days')),
  ('INV-2026-0032',  4, NULL, 15000, 'USD', 'paid',     datetime('now','-9 days'),  datetime('now','-2 days'),  'Patient portal access.',      datetime('now','-9 days')),
  ('INV-2026-0033',  6, NULL, 25000, 'USD', 'issued',   datetime('now','-8 days'),  NULL,                        'Annual membership.',          datetime('now','-8 days')),
  ('INV-2026-0034',  9, NULL,  5000, 'USD', 'draft',    datetime('now','-7 days'),  NULL,                        'Draft — lab fee TBD.',        datetime('now','-7 days')),
  ('INV-2026-0035', 12, NULL, 12000, 'USD', 'paid',     datetime('now','-6 days'),  datetime('now','-1 days'),  'Repeat script.',              datetime('now','-6 days')),
  ('INV-2026-0036', 16, NULL, 40000, 'USD', 'overdue',  datetime('now','-30 days'), NULL,                        'Lab package — overdue.',      datetime('now','-30 days')),
  ('INV-2026-0037', 19, NULL, 22000, 'USD', 'paid',     datetime('now','-5 days'),  datetime('now'),             'Home visit service fee.',     datetime('now','-5 days')),
  ('INV-2026-0038', 25, NULL, 10000, 'USD', 'issued',   datetime('now','-4 days'),  NULL,                        'Physio package.',             datetime('now','-4 days')),
  ('INV-2026-0039', 33, NULL, 45000, 'USD', 'void',     datetime('now','-3 days'),  NULL,                        'Voided — duplicate.',         datetime('now','-3 days')),
  ('INV-2026-0040', 40, NULL,  8000, 'USD', 'draft',    datetime('now','-1 days'),  NULL,                        'Intake fee — not yet issued.', datetime('now','-1 days'));

COMMIT;

-- Summary
SELECT 'departments   ' || (SELECT COUNT(*) FROM departments)   AS counts
UNION ALL SELECT 'doctors       ' || (SELECT COUNT(*) FROM doctors)
UNION ALL SELECT 'patients      ' || (SELECT COUNT(*) FROM patients)
UNION ALL SELECT 'appointments  ' || (SELECT COUNT(*) FROM appointments)
UNION ALL SELECT 'prescriptions ' || (SELECT COUNT(*) FROM prescriptions)
UNION ALL SELECT 'invoices      ' || (SELECT COUNT(*) FROM invoices);

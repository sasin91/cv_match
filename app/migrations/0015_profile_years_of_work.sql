-- The candidate's years of professional work, as they confirmed it on the setup screen's CV step
-- (profile::years_of_work). NULL until they save one: the app then uses the years it counts
-- from the profile's jobs, which the screen offers as the suggestion. Letters, résumés and the
-- match's years check use this number, so a model never counts the years itself.
ALTER TABLE profile ADD COLUMN years_of_work INTEGER DEFAULT NULL;

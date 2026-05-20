PRAGMA defer_foreign_keys = ON;

BEGIN;

PRAGMA defer_foreign_keys = ON;

UPDATE project_board_columns
SET id = 'col_moving-app_in_refinement__tmp'
WHERE id = 'col_moving-app_todo'
  AND project_id = 'moving-app'
  AND name = 'in refinement';

UPDATE work_items
SET column_id = 'col_moving-app_in_refinement__tmp'
WHERE project_id = 'moving-app'
  AND column_id = 'col_moving-app_todo';

UPDATE project_boards
SET movement_guide = replace(movement_guide, 'col_moving-app_todo', 'col_moving-app_in_refinement__tmp')
WHERE project_id = 'moving-app';

UPDATE project_workflows
SET graph = replace(graph, 'col_moving-app_todo', 'col_moving-app_in_refinement__tmp')
WHERE project_id = 'moving-app';

UPDATE project_board_columns
SET id = 'col_moving-app_todo',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 'col_moving-app_backlog'
  AND project_id = 'moving-app'
  AND name = 'TODO';

UPDATE work_items
SET column_id = 'col_moving-app_todo',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app'
  AND column_id = 'col_moving-app_backlog';

UPDATE project_boards
SET movement_guide = replace(movement_guide, 'col_moving-app_backlog', 'col_moving-app_todo'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app';

UPDATE project_workflows
SET graph = replace(graph, 'col_moving-app_backlog', 'col_moving-app_todo'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app';

UPDATE project_board_columns
SET id = 'col_moving-app_in_refinement',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 'col_moving-app_in_refinement__tmp'
  AND project_id = 'moving-app'
  AND name = 'in refinement';

UPDATE work_items
SET column_id = 'col_moving-app_in_refinement',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app'
  AND column_id = 'col_moving-app_in_refinement__tmp';

UPDATE project_boards
SET movement_guide = replace(movement_guide, 'col_moving-app_in_refinement__tmp', 'col_moving-app_in_refinement'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app';

UPDATE project_workflows
SET graph = replace(graph, 'col_moving-app_in_refinement__tmp', 'col_moving-app_in_refinement'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app';

UPDATE project_board_columns
SET id = 'col_moving-app_ready_for_dev',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 'col_moving-app_in_progress'
  AND project_id = 'moving-app'
  AND name = 'Ready for dev';

UPDATE work_items
SET column_id = 'col_moving-app_ready_for_dev',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app'
  AND column_id = 'col_moving-app_in_progress';

UPDATE project_boards
SET movement_guide = replace(movement_guide, 'col_moving-app_in_progress', 'col_moving-app_ready_for_dev'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app';

UPDATE project_workflows
SET graph = replace(graph, 'col_moving-app_in_progress', 'col_moving-app_ready_for_dev'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app';

UPDATE project_boards
SET movement_guide = '{"version":1,"summary":"Four-column workflow: TODO -> in refinement -> Ready for dev -> done.","columnRules":[{"columnId":"col_moving-app_todo","purpose":"Work not yet refined or ready.","moveWhen":"A card is newly created or still needs to be considered."},{"columnId":"col_moving-app_in_refinement","purpose":"Items in refinement.","moveWhen":"A card is being clarified, scoped, or refined."},{"columnId":"col_moving-app_ready_for_dev","purpose":"Ready for dev.","moveWhen":"A card has clear requirements and is ready to be implemented."},{"columnId":"col_moving-app_done","purpose":"Completed work.","moveWhen":"A card is fully completed."}],"transitions":[{"fromColumnId":"col_moving-app_todo","toColumnId":"col_moving-app_in_refinement","when":"Start clarifying or refining the item."},{"fromColumnId":"col_moving-app_in_refinement","toColumnId":"col_moving-app_ready_for_dev","when":"Requirements and acceptance criteria are clear and the item is ready for development."},{"fromColumnId":"col_moving-app_ready_for_dev","toColumnId":"col_moving-app_done","when":"The work has been completed."}],"agentInstructions":"Use the four-column workflow in order. Move items from TODO to in refinement when requirements are being clarified, from in refinement to Ready for dev when scope and acceptance criteria are clear, and to done only when work is complete."}',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE project_id = 'moving-app'
  AND is_default = 1;

COMMIT;

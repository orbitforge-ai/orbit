begin;

set constraints all deferred;

update project_board_columns
set id = 'col_moving-app_in_refinement__tmp'
where id = 'col_moving-app_todo'
  and project_id = 'moving-app'
  and name = 'in refinement';

update work_items
set column_id = 'col_moving-app_in_refinement__tmp'
where project_id = 'moving-app'
  and column_id = 'col_moving-app_todo';

update project_boards
set movement_guide = replace(movement_guide, 'col_moving-app_todo', 'col_moving-app_in_refinement__tmp')
where project_id = 'moving-app';

update project_workflows
set graph = replace(graph, 'col_moving-app_todo', 'col_moving-app_in_refinement__tmp')
where project_id = 'moving-app';

update project_board_columns
set id = 'col_moving-app_todo',
    updated_at = now()::text
where id = 'col_moving-app_backlog'
  and project_id = 'moving-app'
  and name = 'TODO';

update work_items
set column_id = 'col_moving-app_todo',
    updated_at = now()::text
where project_id = 'moving-app'
  and column_id = 'col_moving-app_backlog';

update project_boards
set movement_guide = replace(movement_guide, 'col_moving-app_backlog', 'col_moving-app_todo'),
    updated_at = now()::text
where project_id = 'moving-app';

update project_workflows
set graph = replace(graph, 'col_moving-app_backlog', 'col_moving-app_todo'),
    updated_at = now()::text
where project_id = 'moving-app';

update project_board_columns
set id = 'col_moving-app_in_refinement',
    updated_at = now()::text
where id = 'col_moving-app_in_refinement__tmp'
  and project_id = 'moving-app'
  and name = 'in refinement';

update work_items
set column_id = 'col_moving-app_in_refinement',
    updated_at = now()::text
where project_id = 'moving-app'
  and column_id = 'col_moving-app_in_refinement__tmp';

update project_boards
set movement_guide = replace(movement_guide, 'col_moving-app_in_refinement__tmp', 'col_moving-app_in_refinement'),
    updated_at = now()::text
where project_id = 'moving-app';

update project_workflows
set graph = replace(graph, 'col_moving-app_in_refinement__tmp', 'col_moving-app_in_refinement'),
    updated_at = now()::text
where project_id = 'moving-app';

update project_board_columns
set id = 'col_moving-app_ready_for_dev',
    updated_at = now()::text
where id = 'col_moving-app_in_progress'
  and project_id = 'moving-app'
  and name = 'Ready for dev';

update work_items
set column_id = 'col_moving-app_ready_for_dev',
    updated_at = now()::text
where project_id = 'moving-app'
  and column_id = 'col_moving-app_in_progress';

update project_boards
set movement_guide = replace(movement_guide, 'col_moving-app_in_progress', 'col_moving-app_ready_for_dev'),
    updated_at = now()::text
where project_id = 'moving-app';

update project_workflows
set graph = replace(graph, 'col_moving-app_in_progress', 'col_moving-app_ready_for_dev'),
    updated_at = now()::text
where project_id = 'moving-app';

update project_boards
set movement_guide = '{"version":1,"summary":"Four-column workflow: TODO -> in refinement -> Ready for dev -> done.","columnRules":[{"columnId":"col_moving-app_todo","purpose":"Work not yet refined or ready.","moveWhen":"A card is newly created or still needs to be considered."},{"columnId":"col_moving-app_in_refinement","purpose":"Items in refinement.","moveWhen":"A card is being clarified, scoped, or refined."},{"columnId":"col_moving-app_ready_for_dev","purpose":"Ready for dev.","moveWhen":"A card has clear requirements and is ready to be implemented."},{"columnId":"col_moving-app_done","purpose":"Completed work.","moveWhen":"A card is fully completed."}],"transitions":[{"fromColumnId":"col_moving-app_todo","toColumnId":"col_moving-app_in_refinement","when":"Start clarifying or refining the item."},{"fromColumnId":"col_moving-app_in_refinement","toColumnId":"col_moving-app_ready_for_dev","when":"Requirements and acceptance criteria are clear and the item is ready for development."},{"fromColumnId":"col_moving-app_ready_for_dev","toColumnId":"col_moving-app_done","when":"The work has been completed."}],"agentInstructions":"Use the four-column workflow in order. Move items from TODO to in refinement when requirements are being clarified, from in refinement to Ready for dev when scope and acceptance criteria are clear, and to done only when work is complete."}',
    updated_at = now()::text
where project_id = 'moving-app'
  and is_default = true;

commit;

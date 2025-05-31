// db - is set globally

const PROJECTS_COUNT = 2

const dialog = {
    commands: {
        start: {
            buttons: [
                [{name: {literal: "more_info_btn"}, callback_name: "more_info"}],
                [{name: {literal: "show_projects"}, callback_name: "project_0"}],
            ], // default is `null`
            state: "start"
        },
    },
    buttons: {
        more_info: {
            buttons: [
                [{name: {name: "На главную"}, callback_name: "start"}],
            ]
        },
        start: {
            buttons: [
                [{name: {literal: "more_info_btn"}, callback_name: "more_info"}],
                [{name: {literal: "show_projects"}, callback_name: "project_0"}],
            ], // default is `null`
            replace: true,
            state: "start"
        },
    },
    stateful_msg_handlers: {
        start: {}, // everything is by default, so just send message `start`
        enter_name: {
            // name of the handler function. This field has a side effect:
            // when is set, no automatic sending of message, should be sent
            // manually in handler
            handler: enter_name,
            state: "none"
        },
    },
}

function enter_name() {}

const fmt = (number) => number.toString().padStart(2, '0');

function add_project_callbacks(point) {
    for (const i of Array(PROJECTS_COUNT).keys()) {
        buttons = [
            [],
            [{name: {name: "На главную"}, callback_name: "start"}]
        ]
        if (i > 0) {
            buttons[0].push({name: {literal: "prev_project"}, callback_name: `project_${i-1}`})
        }
        if (i < PROJECTS_COUNT - 1) {
            buttons[0].push({name: {literal: "next_project"}, callback_name: `project_${i+1}`})
        }

        point[`project_${i}`] = {
            replace: true,
            buttons: buttons
        }
    }
}
add_project_callbacks(dialog.buttons)
print(JSON.stringify(dialog.buttons))

const config = {
    version: 1.1,
}

// {config, dialog}
const c = {config: config, dialog: dialog}
c

// db - is set globally

const PROJECTS_COUNT = 2

const start_msg = {
    buttons: [
        [{ name: { literal: "leave_application" }, callback_name: "leave_application" }],
        [{ name: { literal: "more_info_btn" }, callback_name: "more_info" }],
        [{ name: { literal: "ask_question_btn" }, callback_name: "ask_question" }],
    ], // default is `null`
    replace: true,
    state: "start"
};
const dialog = {
    commands: {
        start: {
            meta: true,
            ...start_msg
        },
    },
    buttons: {
        more_info: {
            replace: true,
            buttons: [
                [{ name: { literal: "leave_application" }, callback_name: "leave_application" }],
                [{ name: { literal: "ask_question_btn" }, callback_name: "ask_question" }],
                [{ name: { name: "🏠 На главную" }, callback_name: "start" }],
            ]
        },
        start: start_msg,
        leave_application: {
            literal: "left_application_msg",
            handler: leave_application
        },
        free_doc: {},
        ask_question: {}
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
    variants: {
        start: {
            free_tgads: {
                ...start_msg,
                buttons: [
                    [{ name: { literal: "free_doc_btn" }, callback_name: "free_doc" }],
                    ...start_msg.buttons,
                ],
            },
        },
    },
}

function leave_application(user) {
    print("point of reach")
    user_application(user)

    return false
}

function enter_name() { }

const fmt = (number) => number.toString().padStart(2, '0');

function add_project_callbacks(point) {
    for (const i of Array(PROJECTS_COUNT).keys()) {
        buttons = [
            [],
            [{ name: { name: "На главную" }, callback_name: "start" }]
        ]
        if (i > 0) {
            buttons[0].push({ name: { literal: "prev_project" }, callback_name: `project_${i - 1}` })
        }
        if (i < PROJECTS_COUNT - 1) {
            buttons[0].push({ name: { literal: "next_project" }, callback_name: `project_${i + 1}` })
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
    timezone: 5,
}

const notifications = [
    {
        // time: "17:38",
        time: {once: "17:49"},
        message: {literal: "show_projects"},
    },
    // {
    //     time: {
    //         hour: 0,
    //         delta_minutes: 2,
    //     },
    //     message: {literal: "show_projects"},
    // }
]

// {config, dialog}
const c = { config: config, dialog: dialog, notifications: notifications }
c

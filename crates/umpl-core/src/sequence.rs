//! A sequence diagram: participants side by side, each with a lifeline,
//! and what happens between them, read top to bottom.
//!
//! It is kept as written (PlantUML's sequence diagrams): participants in
//! their order and a list of steps. Positions are not stored; the drawing
//! follows from the order.

/// What a participant is drawn as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ParticipantKind {
    #[default]
    Participant,
    Actor,
    Boundary,
    Control,
    Entity,
    Database,
    Collections,
    Queue,
}

impl ParticipantKind {
    pub const ALL: [ParticipantKind; 8] = [
        ParticipantKind::Participant,
        ParticipantKind::Actor,
        ParticipantKind::Boundary,
        ParticipantKind::Control,
        ParticipantKind::Entity,
        ParticipantKind::Database,
        ParticipantKind::Collections,
        ParticipantKind::Queue,
    ];

    /// The PlantUML keyword.
    pub fn keyword(self) -> &'static str {
        match self {
            ParticipantKind::Participant => "participant",
            ParticipantKind::Actor => "actor",
            ParticipantKind::Boundary => "boundary",
            ParticipantKind::Control => "control",
            ParticipantKind::Entity => "entity",
            ParticipantKind::Database => "database",
            ParticipantKind::Collections => "collections",
            ParticipantKind::Queue => "queue",
        }
    }

    pub fn from_keyword(word: &str) -> Option<ParticipantKind> {
        ParticipantKind::ALL
            .into_iter()
            .find(|k| k.keyword().eq_ignore_ascii_case(word))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Participant {
    /// The name lines use (`A` in `participant "Alice" as A`).
    pub name: String,
    /// The text shown in its box; lines split by `\n`.
    pub shown: String,
    pub kind: ParticipantKind,
    pub stereotype: Option<String>,
    pub color: Option<String>,
    /// Created by a message part way down (`create`, `**`): its box is
    /// drawn where that message arrives, not at the top.
    pub created: bool,
}

/// A coloured box around neighbouring participants (`box … end box`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticipantBox {
    pub title: Option<String>,
    pub color: Option<String>,
    /// The first and last participant inside, by index.
    pub first: usize,
    pub last: usize,
}

/// Where a message starts or ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageEnd {
    Participant(usize),
    /// The left edge of the diagram (`[->`, `?->`).
    Left,
    /// The right edge (`->]`, `->?`).
    Right,
}

/// The arrowhead at the receiving end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Head {
    /// `->`: a filled head (a call).
    #[default]
    Filled,
    /// `->>`: an open head (asynchronous).
    Open,
    /// `-\` or `-/`: half a head.
    Half,
    /// `->x`: the message is lost.
    Lost,
    /// `->o`: a small circle.
    Circle,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Message {
    pub from: Option<MessageEnd>,
    pub to: Option<MessageEnd>,
    pub text: String,
    /// `-->`: a reply.
    pub dashed: bool,
    pub head: Head,
    /// `<->`: heads at both ends.
    pub both: bool,
    pub color: Option<String>,
}

/// Where a note sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteAt {
    Left(usize),
    Right(usize),
    /// Over one participant, or spanning from the first to the second.
    Over(usize, usize),
    /// Across the whole diagram.
    Across,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoteShape {
    #[default]
    Folded,
    /// `hnote`: a hexagon.
    Hexagon,
    /// `rnote`: a plain rectangle.
    Rectangle,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Message(Message),
    Note {
        at: NoteAt,
        text: String,
        shape: NoteShape,
        color: Option<String>,
        /// Next to the step before it rather than below (`/ note`, or a
        /// note right after a message, as PlantUML places it).
        beside_previous: bool,
    },
    /// `alt`, `opt`, `loop`, `par`, `break`, `critical`, `group`.
    FragmentStart {
        kind: String,
        label: String,
        color: Option<String>,
    },
    /// `else`: the next part of the fragment.
    Else {
        label: String,
    },
    FragmentEnd,
    /// `== Section ==`.
    Divider(String),
    /// `...` or `... 5 minutes later ...`.
    Delay(String),
    /// `|||` or `||45||`: empty room, in pixels.
    Space(Option<u32>),
    Activate {
        who: usize,
        color: Option<String>,
    },
    Deactivate {
        who: usize,
    },
    Destroy {
        who: usize,
    },
    /// `ref over A, B : text`.
    Ref {
        first: usize,
        last: usize,
        text: String,
    },
    /// `autonumber` and what follows it (`10`, `10 5 "<b>[000]"`, `stop`,
    /// `resume`): the messages after it are numbered.
    Autonumber(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sequence {
    pub participants: Vec<Participant>,
    pub steps: Vec<Step>,
    pub boxes: Vec<ParticipantBox>,
    /// Participant boxes are drawn again under the lifelines (unless
    /// `hide footbox`).
    pub footbox: bool,
}

impl Sequence {
    pub fn new() -> Sequence {
        Sequence {
            footbox: true,
            ..Sequence::default()
        }
    }

    pub fn participant(&self, name: &str) -> Option<usize> {
        self.participants.iter().position(|p| p.name == name)
    }

    pub fn messages(&self) -> impl Iterator<Item = &Message> {
        self.steps.iter().filter_map(|s| match s {
            Step::Message(m) => Some(m),
            _ => None,
        })
    }
}

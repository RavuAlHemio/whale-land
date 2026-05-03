mod iter_ext;
mod model;
mod output;


use std::num::ParseIntError;
use std::path::PathBuf;

use clap::Parser;
use sxd_document::QName;
use sxd_document::dom::{ChildOfElement, Element};

use crate::iter_ext::SingleIterExt;
use crate::model::{Arg, ArgType, Enum, EnumVariant, Interface, Procedure, Protocol};


#[derive(Parser)]
struct Opts {
    #[arg(short, long = "async")] pub asynchronous: bool,
    #[arg(short, long)] pub in_crate: bool,
    pub xml_proto_input: PathBuf,
}


fn main() {
    let opts = Opts::parse();

    let xml_proto_string = std::fs::read_to_string(&opts.xml_proto_input)
        .expect("failed to read protocol XML");
    let proto_package = sxd_document::parser::parse(&xml_proto_string)
        .expect("failed to parse protocol XML");
    let root_elem = proto_package
        .as_document()
        .root()
        .children()
        .into_iter()
        .filter_map(|n| n.element())
        .single().expect("multiple root elements");
    if root_elem.name() != QName::new("protocol") {
        panic!("root element is not <protocol>");
    }
    let protocol = process_protocol(root_elem);
    let tokenizer = crate::output::Tokenizer::new(opts.asynchronous, opts.in_crate);
    let code_string = tokenizer.protocol_to_code(&protocol);
    println!("{}", code_string);
}

fn process_protocol(protocol_elem: Element<'_>) -> Protocol {
    if protocol_elem.name() != QName::new("protocol") {
        panic!("process_protocol called on non-<protocol> element");
    }

    let name = protocol_elem.attribute_value("name")
        .expect("root <protocol> is missing name=\"...\"")
        .to_owned();

    let mut interfaces = Vec::new();

    let child_elems = protocol_elem
        .children()
        .into_iter()
        .filter_map(|n| n.element());
    for child_elem in child_elems {
        if child_elem.name() == QName::new("copyright") {
            // your interface XML probably falls under a software interoperability exception
            // and you therefore cannot exercise copyright over it
            // and if you have chosen a restrictive license, I laugh at you
            continue;
        } else if child_elem.name() == QName::new("interface") {
            interfaces.push(process_interface(child_elem));
        }
    }

    Protocol {
        name,
        interfaces,
    }
}

fn process_interface(interface_elem: Element<'_>) -> Interface {
    if interface_elem.name() != QName::new("interface") {
        panic!("process_interface called on non-<interface> element");
    }

    let name = interface_elem.attribute_value("name")
        .expect("<interface> is missing name=\"...\"")
        .to_owned();
    let version: u32 = interface_elem.attribute_value("version")
        .expect("<interface> is missing version=\"...\"")
        .parse()
        .expect("<interface> has non-numeric version=\"...\"");

    let mut short_description = None;
    let mut description = None;
    let mut requests = Vec::new();
    let mut events = Vec::new();
    let mut enums = Vec::new();

    let child_elems = interface_elem
        .children()
        .into_iter()
        .filter_map(|n| n.element());
    for child_elem in child_elems {
        if child_elem.name() == QName::new("description") {
            if let Some(summary) = child_elem.attribute_value("summary") {
                short_description = Some(summary.to_owned());
            }
            description = Some(collect_text(child_elem));
        } else if child_elem.name() == QName::new("request") {
            let request = process_procedure(child_elem);
            requests.push(request);
        } else if child_elem.name() == QName::new("event") {
            let event = process_procedure(child_elem);
            events.push(event);
        } else if child_elem.name() == QName::new("enum") {
            let enumeration = process_enum(child_elem);
            enums.push(enumeration);
        }
    }

    Interface {
        name,
        version,
        short_description,
        description,
        requests,
        events,
        enums,
    }
}

fn process_procedure(proc_elem: Element<'_>) -> Procedure {
    if proc_elem.name() != QName::new("request") && proc_elem.name() != QName::new("event") {
        panic!("process_procedure called on non-<request>, non-<event> element");
    }

    let name = proc_elem.attribute_value("name")
        .expect("<request>/<event> without name=\"...\"")
        .to_owned();
    let mut short_description = None;
    let mut description = None;
    let mut args = Vec::new();

    let child_elems = proc_elem
        .children()
        .into_iter()
        .filter_map(|n| n.element());
    for child_elem in child_elems {
        if child_elem.name() == QName::new("description") {
            if let Some(summary) = child_elem.attribute_value("summary") {
                short_description = Some(summary.to_owned());
            }
            description = Some(collect_text(child_elem));
        } else if child_elem.name() == QName::new("arg") {
            let arg = process_arg(child_elem);
            args.push(arg);
        }
    }

    Procedure {
        name,
        short_description,
        description,
        args,
    }
}

fn process_arg(arg_elem: Element<'_>) -> Arg {
    if arg_elem.name() != QName::new("arg") {
        panic!("process_arg called on non-<arg> element");
    }

    let name = arg_elem.attribute_value("name")
        .expect("<arg> without name=\"...\"")
        .to_owned();
    let arg_type = ArgType::try_from_str(
        arg_elem.attribute_value("type")
            .expect("<arg> without type=\"...\"")
    ).expect("<arg> with unknown type=\"...\"");
    let interface = arg_elem.attribute_value("interface")
        .map(|i| i.to_owned());
    let short_description = arg_elem.attribute_value("summary")
        .map(|sd| sd.to_owned());

    Arg {
        name,
        arg_type,
        interface,
        short_description,
    }
}

fn process_enum(enum_elem: Element<'_>) -> Enum {
    if enum_elem.name() != QName::new("enum") {
        panic!("process_enum called on non-<enum> element");
    }

    let name = enum_elem.attribute_value("name")
        .expect("<enum> without name=\"...\"")
        .to_owned();
    let mut short_description = None;
    let mut description = None;
    let mut variants = Vec::new();

    let child_elems = enum_elem
        .children()
        .into_iter()
        .filter_map(|n| n.element());
    for child_elem in child_elems {
        if child_elem.name() == QName::new("description") {
            if let Some(summary) = child_elem.attribute_value("summary") {
                short_description = Some(summary.to_owned());
            }
            description = Some(collect_text(child_elem));
        } else if child_elem.name() == QName::new("entry") {
            let variant = process_enum_variant(child_elem);
            variants.push(variant);
        }
    }

    Enum {
        name,
        short_description,
        description,
        variants,
    }
}

fn process_enum_variant(variant_elem: Element<'_>) -> EnumVariant {
    if variant_elem.name() != QName::new("entry") {
        panic!("process_enum_variant called on non-<entry> element");
    }

    let name = variant_elem.attribute_value("name")
        .expect("<entry> without name=\"...\"")
        .to_owned();
    let value_str = variant_elem.attribute_value("value")
        .expect("<entry> without value=\"...\"");
    let value = parse_u32_base_prefix(value_str)
        .expect("<entry> with non-u32 value=\"...\"");
    let short_description = variant_elem.attribute_value("summary")
        .map(|s| s.to_owned());

    EnumVariant {
        name,
        value,
        short_description,
    }
}

fn collect_text_recurse(elem: Element<'_>, string: &mut String) {
    for child in elem.children() {
        match child {
            ChildOfElement::Element(child_elem) => {
                collect_text_recurse(child_elem, string);
            },
            ChildOfElement::Text(text) => {
                string.push_str(text.text());
            },
            ChildOfElement::Comment(_) => {},
            ChildOfElement::ProcessingInstruction(_) => {},
        }
    }
}
fn collect_text(elem: Element<'_>) -> String {
    let mut ret = String::new();
    collect_text_recurse(elem, &mut ret);
    ret
}

fn parse_u32_base_prefix(int_str: &str) -> Result<u32, ParseIntError> {
    if let Some(hex) = int_str.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
    } else if let Some(bin) = int_str.strip_prefix("0b") {
        u32::from_str_radix(bin, 2)
    } else {
        u32::from_str_radix(int_str, 10)
    }
}

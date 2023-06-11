extern crate proc_macro;

use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use regex::Regex;
use syn::{
    parse_macro_input, Attribute, Data::Enum, Data::Struct, DataEnum, DataStruct, DeriveInput,
    Fields, Ident, Lit, Meta, NestedMeta,
};

#[proc_macro_derive(Recap, attributes(recap))]
pub fn derive_recap(item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as DeriveInput);
    let regex = extract_regex(&item).expect(
        r#"Unable to resolve recap regex.
            Make sure your structure has declared an attribute in the form:
            #[derive(Deserialize, Recap)]
            #[recap(regex ="your-pattern-here")]
            struct YourStruct { ... }
            "#,
    );

    validate(&item, &regex);

    let item_ident = &item.ident;
    let (impl_generics, ty_generics, where_clause) = item.generics.split_for_impl();

    let has_lifetimes = item.generics.lifetimes().count() > 0;
    let (impl_inner, impl_matcher) = match regex {
        Regexes::StructRegex(regex) => {
            let impl_from_str = if !has_lifetimes {
                quote! {
                    impl #impl_generics std::str::FromStr for #item_ident #ty_generics #where_clause {
                        type Err = recap::Error;
                        fn from_str(s: &str) -> Result<Self, Self::Err> {
                            recap::lazy_static! {
                                static ref RE: recap::Regex = recap::Regex::new(#regex)
                                    .expect("Failed to compile regex");
                            }

                            recap::from_captures(&RE, s)
                        }
                    }
                }
            } else {
                quote! {}
            };

            let lifetimes = item.generics.lifetimes();
            let also_lifetimes = item.generics.lifetimes();
            let impl_inner = quote! {
                impl #impl_generics std::convert::TryFrom<& #(#lifetimes)* str> for #item_ident #ty_generics #where_clause {
                    type Error = recap::Error;
                    fn try_from(s: & #(#also_lifetimes)* str) -> Result<Self, Self::Error> {
                        recap::lazy_static! {
                            static ref RE: recap::Regex = recap::Regex::new(#regex)
                                .expect("Failed to compile regex");
                        }

                        recap::from_captures(&RE, s)
                    }
                }
                #impl_from_str
            };

            let impl_matcher = quote! {
                impl #impl_generics  #item_ident #ty_generics #where_clause {
                    /// Recap derived method. Returns true when some input text
                    /// matches the regex associated with this type
                    pub fn is_match(input: &str) -> bool {
                        recap::lazy_static! {
                            static ref RE: recap::Regex = recap::Regex::new(#regex)
                                .expect("Failed to compile regex");
                        }
                        RE.is_match(input)
                    }
                }
            };
            (impl_inner, impl_matcher)
        }
        Regexes::EnumRegexes(regexes) => {
            let data_enum = match item.data {
                Enum(data_enum) => data_enum,
                _ => panic!("expected Enum"),
            };

            let build_regex_name_ident = |variant_name: String| {
                Ident::new(&format!("RE_{}", variant_name), Span::call_site())
            };

            let static_regexes: Vec<_> = regexes
                .iter()
                .map(|(variant_name, regex)| {
                    let regex_name = build_regex_name_ident(variant_name.to_string());
                    quote! {
                            static ref #regex_name: recap::Regex = recap::Regex::new(#regex)
                                .expect("Failed to compile regex");
                    }
                })
                .collect();

            let parsers =
                data_enum.variants.iter().map(|variant| {
                    let variant_name = &variant.ident;
                    let name = &item.ident;
                    let regex_name = build_regex_name_ident(variant_name.to_string());
                    match &variant.fields {
                        Fields::Named(fields) => {
                            let fields: Vec<Ident> = fields.named.iter().map(|f| f.ident.clone().unwrap()).collect();
                            quote! {
                                if let Some(caps) = #regex_name.captures(&s) {
                                    return Ok(#name::#variant_name {
                                        #(#fields: caps.name(stringify!(#fields)).unwrap().as_str().parse().unwrap(),)*
                                    })
                                }
                            }
                        }
                        Fields::Unnamed(_) => {
                            quote! {
                                if let Some(caps) = #regex_name.captures(&s) {
                                    let inner = caps.get(1).unwrap().as_str();
                                    if let Ok(value) = inner.parse() {
                                        return Ok(#name::#variant_name(value))
                                    }
                                }
                            }
                        }
                        Fields::Unit => {
                            quote! {
                                if #regex_name.is_match(&s) {
                                    return Ok(#name::#variant_name)
                                }
                            }
                        }
                }}).collect::<Vec<_>>();

            let matchers = regexes.keys().map(|variant_name| {
                let regex = build_regex_name_ident(variant_name.to_string());
                quote! {
                    if #regex.is_match(input) {
                        return true;
                    };
                }
            });

            let impl_from_str = if !has_lifetimes {
                quote! {
                    impl #impl_generics std::str::FromStr for #item_ident #ty_generics #where_clause {
                        type Err = recap::Error;
                        fn from_str(s: &str) -> Result<Self, Self::Err> {
                            recap::lazy_static! {
                                #(#static_regexes)*
                            }

                            #(#parsers)*

                            Err(Self::Err::Custom("Uh Oh".to_string()))
                        }
                    }
                }
            } else {
                quote! {}
            };

            let lifetimes = item.generics.lifetimes();
            let also_lifetimes = item.generics.lifetimes();
            let impl_inner = {
                quote! {
                    impl #impl_generics std::convert::TryFrom<& #(#lifetimes)* str> for #item_ident #ty_generics #where_clause {
                        type Error = recap::Error;
                        fn try_from(s: & #(#also_lifetimes)* str) -> Result<Self, Self::Error> {
                            recap::lazy_static! {
                                #(#static_regexes)*
                            }
                            #(#parsers)*

                            Err(Self::Error::Custom("Uh Oh".to_string()))
                        }
                    }
                    #impl_from_str
                }
            };

            let impl_matcher = {
                quote! {
                impl #impl_generics  #item_ident #ty_generics #where_clause {
                    /// Recap derived method. Returns true when some input text
                    /// matches the regex associated with this type
                    pub fn is_match(input: &str) -> bool {
                            recap::lazy_static! {
                                #(#static_regexes)*
                            }
                            #(#matchers)*
                            false
                        }
                    }
                }
            };

            (impl_inner, impl_matcher)
        }
    };

    let injector = Ident::new(
        &format!("RECAP_IMPL_FOR_{}", item.ident.to_string().to_uppercase()),
        Span::call_site(),
    );

    let out = quote! {
        const #injector: () = {
            extern crate recap;
            #impl_inner
            #impl_matcher
        };
    };
    out.into()
}

enum Regexes {
    StructRegex(String),
    EnumRegexes(HashMap<String, String>),
}

fn validate(
    item: &DeriveInput,
    regex_container: &Regexes,
) {
    match regex_container {
        Regexes::StructRegex(regex) => validate_struct(item, regex),
        Regexes::EnumRegexes(regexes) => validate_enum(item, regexes),
    }
}
fn validate_enum(
    item: &DeriveInput,
    regexes: &HashMap<String, String>,
) {
    if let Enum(DataEnum { variants, .. }) = &item.data {
        for variant in variants {
            let variant_name = format!("{}", variant.ident);
            let regex = Regex::new(regexes.get(&variant_name).unwrap()).unwrap_or_else(|err| {
                panic!(
                    "Invalid regular expression provided for `{}`\n{}",
                    &item.ident, err
                )
            });
            match &variant.fields {
                Fields::Named(_) | Fields::Unnamed(_) => {
                    let caps = regex.capture_names().flatten().count();
                    let fields = variant.fields.len();
                    if caps != fields {
                        panic!(
                            "Recap could not derive a `FromStr` impl for `{}`.\n\t\t > Expected regex with {} named capture groups to align with struct fields but found {}",
                            item.ident, fields, caps
                        );
                    }
                }
                Fields::Unit => {}
            };
        }
    };
}

fn validate_struct(
    item: &DeriveInput,
    regex: &str,
) {
    let regex = Regex::new(regex).unwrap_or_else(|err| {
        panic!(
            "Invalid regular expression provided for `{}`\n{}",
            &item.ident, err
        )
    });
    let caps = regex.capture_names().flatten().count();
    let fields = match &item.data {
        Struct(DataStruct {
            fields: Fields::Named(fs),
            ..
        }) => fs.named.len(),
        _ => {
            panic!("Recap regex can only be applied to Structs and Enums with named fields")
        }
    };
    if caps != fields {
        panic!(
            "Recap could not derive a `FromStr` impl for `{}`.\n\t\t > Expected regex with {} named capture groups to align with struct fields but found {}",
            item.ident, fields, caps
        );
    }
}

fn extract_regex(item: &DeriveInput) -> Option<Regexes> {
    match &item.data {
        Struct(_) => extract_regex_from_recap_attribute(&item.attrs).map(Regexes::StructRegex),
        Enum(data_enum) => Some(Regexes::EnumRegexes(extract_enum_regexes(data_enum))),
        _ => None,
    }
}

fn extract_regex_from_recap_attribute(attrs: &[Attribute]) -> Option<String> {
    attrs
        .iter()
        .flat_map(syn::Attribute::parse_meta)
        .filter_map(|x| match x {
            Meta::List(y) => Some(y),
            _ => None,
        })
        .filter(|x| x.path.is_ident("recap"))
        .flat_map(|x| x.nested.into_iter())
        .filter_map(|x| match x {
            NestedMeta::Meta(y) => Some(y),
            _ => None,
        })
        .filter_map(|x| match x {
            Meta::NameValue(y) => Some(y),
            _ => None,
        })
        .find(|x| x.path.is_ident("regex"))
        .and_then(|x| match x.lit {
            Lit::Str(y) => Some(y.value()),
            _ => None,
        })
}

fn extract_enum_regexes(data_enum: &DataEnum) -> HashMap<String, String> {
    data_enum
        .variants
        .iter()
        .map(|variant| {
            let regex = extract_regex_from_recap_attribute(&variant.attrs).unwrap();
            (format!("{}", variant.ident), regex)
        })
        .collect()
}

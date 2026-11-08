//! A component written once: its struct is its schema (PLAN-property-types §5).
//!
//! `#[derive(Component)]` on a struct writes the property specs, a property a
//! field in field order, each field's doc comment as its `description`; the
//! struct's `Default`, from each field's `#[property(default = ...)]`; and a
//! read and a write of one property by its index. `#[derive(Record)]` is the
//! same for a struct a property holds, `#[derive(Enum)]` makes a fieldless
//! enum an `enum` property whose options are its variants, and
//! `#[derive(Variant)]` makes an enum whose every variant holds a record a
//! `variant` property: the record of the variant its `kind` names.
//!
//! What the generated code reaches for lives in
//! `balaur_core::components::derived`, so a crate deriving needs `balaur_core`
//! and nothing else.

use crate::vocabulary::words;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Expr, ExprLit, Fields, Lit, Meta, UnOp};

const OPTION: &str = "option";
mod vocabulary;

/// `#[derive(Component)]`: see the crate docs. Needs `#[component(name = "...")]`.
#[proc_macro_derive(Component, attributes(component, property))]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    expand(&input, true)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[derive(Record)]`: a struct a property holds, written as a table.
#[proc_macro_derive(Record, attributes(property))]
pub fn derive_record(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    expand(&input, false)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[derive(Enum)]`: a fieldless enum as an `enum` property, each variant
/// an option in snake case unless `#[option(name = "...")]` names it.
#[proc_macro_derive(Enum, attributes(option))]
pub fn derive_enum(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    expand_enum(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// `#[derive(Variant)]`: an enum whose every variant holds one record, as a
/// `variant` property. Each variant is named in snake case unless
/// `#[option(name = "...")]` names it; `#[option(default)]` marks the one
/// `Default` gives, the first otherwise.
#[proc_macro_derive(Variant, attributes(option))]
pub fn derive_variant(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    expand_variant(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// The text of a run of `#[doc]` attributes: lines joined with a space, a
/// blank line kept as a paragraph break.
fn doc_of(attrs: &[syn::Attribute]) -> String {
    let mut out = String::new();
    for attr in attrs {
        let Meta::NameValue(pair) = &attr.meta else {
            continue;
        };
        if !pair.path.is_ident("doc") {
            continue;
        }
        let Expr::Lit(ExprLit {
            lit: Lit::Str(text),
            ..
        }) = &pair.value
        else {
            continue;
        };
        let line = text.value();
        let line = line.trim();
        if line.is_empty() {
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push_str("\n\n");
            }
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            out.push_str(line);
        }
    }
    out.trim_end().to_owned()
}

/// A `snake_case` name for a `CamelCase` one.
fn snake(name: &str) -> String {
    let mut out = String::new();
    for (at, letter) in name.char_indices() {
        if letter.is_uppercase() {
            if at > 0 {
                out.push('_');
            }
            out.extend(letter.to_lowercase());
        } else {
            out.push(letter);
        }
    }
    out
}

/// A spec key's value, written as the TOML value the spec holds.
fn spec_value(value: &Expr) -> syn::Result<Tokens> {
    let toml = quote!(::balaur_core::components::derived::toml);
    Ok(match value {
        Expr::Lit(ExprLit { lit, .. }) => match lit {
            Lit::Str(text) => quote!(#toml::Value::String(::std::string::String::from(#text))),
            Lit::Int(number) => quote!(#toml::Value::Integer(#number)),
            Lit::Float(number) => quote!(#toml::Value::Float(#number)),
            Lit::Bool(flag) => quote!(#toml::Value::Boolean(#flag)),
            other => {
                return Err(syn::Error::new(
                    other.span(),
                    "a spec takes text, a number or a flag",
                ));
            }
        },
        Expr::Unary(negative) if matches!(negative.op, UnOp::Neg(_)) => match &*negative.expr {
            Expr::Lit(ExprLit {
                lit: Lit::Int(number),
                ..
            }) => quote!(#toml::Value::Integer(-#number)),
            Expr::Lit(ExprLit {
                lit: Lit::Float(number),
                ..
            }) => quote!(#toml::Value::Float(-#number)),
            other => return Err(syn::Error::new(other.span(), "only a number takes a minus")),
        },
        Expr::Array(list) => {
            let entries = list
                .elems
                .iter()
                .map(spec_value)
                .collect::<syn::Result<Vec<_>>>()?;
            quote!(#toml::Value::Array(::std::vec![#(#entries),*]))
        }
        other => {
            return Err(syn::Error::new(
                other.span(),
                "a spec takes text, a number, a flag or a list of them",
            ));
        }
    })
}

/// One field's `#[property(...)]`, read.
#[derive(Default)]
struct PropertyAttrs {
    /// The property's name where it is not the field's.
    name: Option<String>,
    /// What a new component starts at, as a Rust expression of the field's
    /// type; a text literal is turned into the field's type with `into`.
    default: Option<Expr>,
    /// Every other key, written into the spec as it stands.
    keys: Vec<(String, Expr)>,
}

fn property_attrs(field: &syn::Field) -> syn::Result<PropertyAttrs> {
    let mut found = PropertyAttrs::default();
    for attr in &field.attrs {
        if !attr.path().is_ident("property") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .map(ToString::to_string)
                .ok_or_else(|| meta.error("a property key is one word"))?;
            let key = key.trim_start_matches("r#").to_owned();
            if !meta.input.peek(syn::Token![=]) {
                // A bare word names the type where the Rust one does not say
                // it (`color` on four floats, `node` on a path), or is a
                // flag: `readonly`.
                match key.as_str() {
                    words::COLOR | words::NODE | words::FLAGS => {
                        found
                            .keys
                            .push((words::TYPE.to_owned(), syn::parse_quote!(#key)));
                    }
                    _ => found.keys.push((key, syn::parse_quote!(true))),
                }
                return Ok(());
            }
            let value: Expr = meta.value()?.parse()?;
            match key.as_str() {
                words::NAME => match &value {
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(text),
                        ..
                    }) => found.name = Some(text.value()),
                    _ => return Err(meta.error("`name` takes text")),
                },
                words::DEFAULT => found.default = Some(value),
                words::DESCRIPTION => {
                    return Err(meta.error("a property's description is its doc comment"));
                }
                _ => found.keys.push((key, value)),
            }
            Ok(())
        })?;
    }
    // A property naming its asset type is an asset.
    if found.keys.iter().any(|(key, _)| key == words::ASSET)
        && !found.keys.iter().any(|(key, _)| key == words::TYPE)
    {
        found
            .keys
            .push((words::TYPE.to_owned(), syn::parse_quote!("asset")));
    }
    Ok(found)
}

/// The lifecycle points `#[component(..)]` may name a function for.
const LIFECYCLE_POINTS: [&str; 5] = [
    "on_add",
    "on_insert",
    "on_replace",
    "on_remove",
    "on_despawn",
];

/// What `#[component(..)]` says beside the name.
struct ComponentAttrs {
    name: String,
    /// Each lifecycle point named, with its function.
    lifecycle: Vec<(syn::Ident, syn::Path)>,
    /// `requires = [..]`: the names of the components this one brings.
    requires: Vec<syn::Expr>,
    /// `tags = [..]`: the facets it is browsed under.
    tags: Vec<syn::Expr>,
    /// `expects = [..]`: components it needs something from.
    expects: Vec<syn::Expr>,
    /// `events = ..`: what it announces from its node.
    events: Option<syn::Expr>,
    /// `after_write = path`: what it does once a write has landed.
    after_write: Option<syn::Path>,
    /// `warnings = path`: what it says is off about it on a node.
    warnings: Option<syn::Path>,
}

/// `#[component(name = "...", on_add = path, requires = [..], ..)]`, read.
fn component_attrs(input: &DeriveInput) -> syn::Result<ComponentAttrs> {
    let mut name = None;
    let mut lifecycle = Vec::new();
    let mut requires = Vec::new();
    let mut tags = Vec::new();
    let mut expects = Vec::new();
    let mut events = None;
    let mut after_write = None;
    let mut warnings = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("component") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident(words::NAME) {
                let text: syn::LitStr = meta.value()?.parse()?;
                name = Some(text.value());
                Ok(())
            } else if meta.path.is_ident("requires") {
                let list: syn::ExprArray = meta.value()?.parse()?;
                requires.extend(list.elems);
                Ok(())
            } else if meta.path.is_ident("tags") {
                let list: syn::ExprArray = meta.value()?.parse()?;
                tags.extend(list.elems);
                Ok(())
            } else if meta.path.is_ident("expects") {
                let list: syn::ExprArray = meta.value()?.parse()?;
                expects.extend(list.elems);
                Ok(())
            } else if meta.path.is_ident("events") {
                events = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("after_write") {
                after_write = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("warnings") {
                warnings = Some(meta.value()?.parse()?);
                Ok(())
            } else if let Some(point) = LIFECYCLE_POINTS
                .iter()
                .find(|point| meta.path.is_ident(point))
            {
                let call: syn::Path = meta.value()?.parse()?;
                lifecycle.push((syn::Ident::new(point, meta.path.span()), call));
                Ok(())
            } else {
                Err(meta.error(
                    "`component` takes `name`, `requires`, `tags`, `expects`, `events`, `after_write`, `warnings`, `on_add`, `on_insert`, `on_replace`, `on_remove` and `on_despawn`",
                ))
            }
        })?;
    }
    let name = name.ok_or_else(|| {
        syn::Error::new(
            input.ident.span(),
            "a component names itself: #[component(name = \"...\")]",
        )
    })?;
    Ok(ComponentAttrs {
        name,
        lifecycle,
        requires,
        tags,
        expects,
        events,
        after_write,
        warnings,
    })
}

/// `const LIFECYCLE` naming each point the attribute named, nothing when it
/// named none, so the trait's empty default stands.
fn lifecycle_const(derived: &Tokens, lifecycle: &[(syn::Ident, syn::Path)]) -> Tokens {
    if lifecycle.is_empty() {
        return quote! {};
    }
    let points = lifecycle.iter().map(|(point, call)| {
        quote! { #point: ::core::option::Option::Some(#call), }
    });
    // Every point named leaves nothing for the empty default to fill.
    let rest =
        (lifecycle.len() < LIFECYCLE_POINTS.len()).then(|| quote! { ..#derived::Lifecycle::NONE });
    quote! {
        const LIFECYCLE: #derived::Lifecycle = #derived::Lifecycle {
            #(#points)*
            #rest
        };
    }
}

/// What one field adds to each generated item.
struct FieldParts {
    name: String,
    default: Tokens,
    spec: Tokens,
    read: Tokens,
    given: Tokens,
    write: Tokens,
}

fn field_parts(index: usize, field: &syn::Field) -> syn::Result<FieldParts> {
    let derived = quote!(::balaur_core::components::derived);
    let toml = quote!(#derived::toml);
    let field_ident = field.ident.as_ref().expect("named fields have names");
    let field_type = &field.ty;
    let attrs = property_attrs(field)?;
    let name = attrs
        .name
        .clone()
        .unwrap_or_else(|| field_ident.to_string().trim_start_matches("r#").to_owned());
    let default = match &attrs.default {
        Some(Expr::Lit(ExprLit {
            lit: Lit::Str(text),
            ..
        })) => quote!(#field_ident: ::core::convert::Into::into(#text)),
        Some(value) => quote!(#field_ident: #value),
        None => quote!(#field_ident: ::core::default::Default::default()),
    };
    let description = doc_of(&field.attrs);
    let mut keys = Vec::new();
    if !description.is_empty() {
        keys.push(quote!(spec.insert(
            ::std::string::String::from("description"),
            #toml::Value::String(::std::string::String::from(#description)),
        );));
    }
    for (key, value) in &attrs.keys {
        let value = spec_value(value)?;
        keys.push(quote!(spec.insert(::std::string::String::from(#key), #value);));
    }
    let spec = quote!({
        let mut spec = #toml::map::Map::new();
        #(#keys)*
        <#field_type as #derived::Property>::describe(&mut spec);
        spec.insert(
            ::std::string::String::from("default"),
            #derived::Property::to_value(&start.#field_ident),
        );
        #toml::Value::Table(spec)
    });
    let read = quote!(#index => ::core::option::Option::Some(#derived::Property::to_value(&self.#field_ident)),);
    let given = quote!(#index => #derived::Property::given(&self.#field_ident),);
    let write = quote!(#index => {
        self.#field_ident = #derived::Property::from_given(value)
            .map_err(|why| why.context(#name))?;
    });
    Ok(FieldParts {
        name,
        default,
        spec,
        read,
        given,
        write,
    })
}

/// The names in the order a table's keys sort in, each with its field index,
/// so a table is read in one walk rather than a lookup per field.
fn by_name(names: &[String]) -> Vec<Tokens> {
    let mut sorted: Vec<(&String, usize)> = names.iter().zip(0usize..).collect();
    sorted.sort();
    sorted
        .into_iter()
        .map(|(name, index)| quote!((#name, #index)))
        .collect()
}

/// The struct's named fields, or the error a derive on anything else is.
fn named_fields(input: &DeriveInput) -> syn::Result<&syn::FieldsNamed> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "only a struct derives this",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new(
            input.ident.span(),
            "only a struct with named fields derives this",
        ));
    };
    Ok(fields)
}

/// `impl Component`: the name and the doc, and each const the attribute named.
fn component_impl(input: &DeriveInput, derived: &Tokens) -> syn::Result<Tokens> {
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let ComponentAttrs {
        name,
        lifecycle,
        requires,
        tags,
        expects,
        events,
        after_write,
        warnings,
    } = component_attrs(input)?;
    let doc = doc_of(&input.attrs);
    let lifecycle = lifecycle_const(derived, &lifecycle);
    let requires = if requires.is_empty() {
        quote! {}
    } else {
        quote! { const REQUIRES: &'static [&'static str] = &[#(#requires),*]; }
    };
    let tags =
        (!tags.is_empty()).then(|| quote! { const TAGS: &'static [&'static str] = &[#(#tags),*]; });
    let expects = (!expects.is_empty())
        .then(|| quote! { const EXPECTS: &'static [&'static str] = &[#(#expects),*]; });
    let events = events.map(|events| {
        quote! { const EVENTS: &'static [(&'static str, &'static str)] = #events; }
    });
    let after_write = after_write.map(|call| {
        quote! {
            const AFTER_WRITE: ::core::option::Option<#derived::AfterWriteFn> =
                ::core::option::Option::Some(#call);
        }
    });
    let warnings = warnings.map(|call| {
        quote! {
            const WARNINGS: ::core::option::Option<#derived::WarningFn> =
                ::core::option::Option::Some(#call);
        }
    });
    Ok(quote! {
        impl #impl_generics #derived::Component for #ident #type_generics #where_clause {
            const NAME: &'static str = #name;
            const DOC: &'static str = #doc;
            #requires
            #tags
            #expects
            #events
            #after_write
            #warnings
            #lifecycle
        }
    })
}

fn expand(input: &DeriveInput, component: bool) -> syn::Result<Tokens> {
    let fields = named_fields(input)?;
    let derived = quote!(::balaur_core::components::derived);
    let toml = quote!(#derived::toml);
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();

    let mut names = Vec::new();
    let mut defaults = Vec::new();
    let mut specs = Vec::new();
    let mut reads = Vec::new();
    let mut givens = Vec::new();
    let mut writes = Vec::new();
    for (index, field) in fields.named.iter().enumerate() {
        let parts = field_parts(index, field)?;
        names.push(parts.name);
        defaults.push(parts.default);
        specs.push(parts.spec);
        reads.push(parts.read);
        givens.push(parts.given);
        writes.push(parts.write);
    }
    let count = names.len();
    let by_name = by_name(&names);

    let fields_impl = quote! {
        impl #impl_generics ::core::default::Default for #ident #type_generics #where_clause {
            fn default() -> Self {
                Self { #(#defaults),* }
            }
        }

        impl #impl_generics #derived::Fields for #ident #type_generics #where_clause {
            const NAMES: &'static [&'static str] = &[#(#names),*];
            const BY_NAME: &'static [(&'static str, usize)] = &[#(#by_name),*];

            fn specs() -> ::std::vec::Vec<#toml::Value> {
                let start = <Self as ::core::default::Default>::default();
                let mut out = ::std::vec::Vec::with_capacity(#count);
                #(out.push(#specs);)*
                out
            }

            fn read(&self, index: usize) -> ::core::option::Option<#toml::Value> {
                match index {
                    #(#reads)*
                    _ => ::core::option::Option::None,
                }
            }

            fn given(&self, index: usize) -> ::core::option::Option<#derived::Given<'_>> {
                match index {
                    #(#givens)*
                    _ => ::core::option::Option::None,
                }
            }

            fn write(&mut self, index: usize, value: #derived::Given<'_>) -> #derived::Result<()> {
                match index {
                    #(#writes)*
                    _ => return ::core::result::Result::Err(#derived::no_property(index)),
                }
                ::core::result::Result::Ok(())
            }
        }
    };

    let kind_impl = if component {
        component_impl(input, &derived)?
    } else {
        quote! {
            impl #impl_generics #derived::Property for #ident #type_generics #where_clause {
                fn describe(spec: &mut #toml::map::Map<::std::string::String, #toml::Value>) {
                    #derived::describe_record::<Self>(spec);
                }

                fn to_value(&self) -> #toml::Value {
                    #derived::to_table(self)
                }

                fn from_value(value: &#toml::Value) -> #derived::Result<Self> {
                    #derived::from_table(value)
                }
            }
        }
    };
    Ok(quote! {
        #fields_impl
        #kind_impl
    })
}

fn expand_enum(input: &DeriveInput) -> syn::Result<Tokens> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "only an enum derives `Enum`",
        ));
    };
    let derived = quote!(::balaur_core::components::derived);
    let toml = quote!(#derived::toml);
    let ident = &input.ident;
    let mut options = Vec::new();
    let mut variants = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new(variant.span(), "an option holds nothing"));
        }
        let mut name = snake(&variant.ident.to_string());
        for attr in &variant.attrs {
            if !attr.path().is_ident(OPTION) {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident(words::NAME) {
                    let text: syn::LitStr = meta.value()?.parse()?;
                    name = text.value();
                    Ok(())
                } else {
                    Err(meta.error("`option` takes `name`"))
                }
            })?;
        }
        options.push(name);
        variants.push(&variant.ident);
    }
    Ok(quote! {
        impl #ident {
            /// Every option, in the order the enum declares them.
            pub const OPTIONS: &'static [&'static str] = &[#(#options),*];

            /// The option's name, as a file and a script spell it.
            #[must_use]
            pub const fn option(&self) -> &'static str {
                match self {
                    #(Self::#variants => #options,)*
                }
            }
        }

        impl #derived::Property for #ident {
            fn describe(spec: &mut #toml::map::Map<::std::string::String, #toml::Value>) {
                #derived::describe_enum(spec, Self::OPTIONS);
            }

            fn to_value(&self) -> #toml::Value {
                #toml::Value::String(::std::string::String::from(self.option()))
            }

            fn from_value(value: &#toml::Value) -> #derived::Result<Self> {
                match value.as_str() {
                    #(::core::option::Option::Some(#options) => ::core::result::Result::Ok(Self::#variants),)*
                    _ => ::core::result::Result::Err(#derived::not_an_option(value, Self::OPTIONS)),
                }
            }

            fn from_given(given: #derived::Given<'_>) -> #derived::Result<Self> {
                match given {
                    #(#derived::Given::Text(#options) => ::core::result::Result::Ok(Self::#variants),)*
                    other => #derived::from_given(other),
                }
            }

            fn given(&self) -> ::core::option::Option<#derived::Given<'_>> {
                ::core::option::Option::Some(#derived::Given::Text(self.option()))
            }
        }
    })
}

/// The match arms a variant enum's `Property` impl needs, one per variant:
/// the pattern naming it, its spec, its value, and the read of one.
struct VariantArms {
    patterns: Vec<Tokens>,
    describes: Vec<Tokens>,
    to_values: Vec<Tokens>,
    from_values: Vec<Tokens>,
}

/// Each variant's arms: one holding a record reads and writes its fields,
/// one holding nothing is its name alone.
fn variant_arms(
    derived: &Tokens,
    variants: &[&syn::Ident],
    records: &[Option<&syn::Type>],
    options: &[String],
    docs: &[String],
) -> VariantArms {
    let mut arms = VariantArms {
        patterns: Vec::new(),
        describes: Vec::new(),
        to_values: Vec::new(),
        from_values: Vec::new(),
    };
    for (((ident, record), option), doc) in variants.iter().zip(records).zip(options).zip(docs) {
        if let Some(record) = record {
            arms.patterns.push(quote!(Self::#ident(_)));
            arms.describes
                .push(quote!(#derived::variant_record::<#record>(#doc)));
            arms.to_values
                .push(quote!(Self::#ident(held) => #derived::variant_value(#option, held)));
            arms.from_values.push(quote!(#option => ::core::result::Result::Ok(Self::#ident(#derived::from_table(value)?))));
        } else {
            arms.patterns.push(quote!(Self::#ident));
            arms.describes.push(quote!(#derived::variant_unit(#doc)));
            arms.to_values
                .push(quote!(Self::#ident => #derived::variant_name(#option)));
            arms.from_values
                .push(quote!(#option => ::core::result::Result::Ok(Self::#ident)));
        }
    }
    arms
}

/// A variant's name, snake case unless `#[option(name = "...")]` says, and
/// whether `#[option(default)]` marks it.
fn variant_option(variant: &syn::Variant) -> syn::Result<(String, bool)> {
    let mut name = snake(&variant.ident.to_string());
    let mut chosen = false;
    for attr in &variant.attrs {
        if !attr.path().is_ident(OPTION) {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident(words::NAME) {
                let text: syn::LitStr = meta.value()?.parse()?;
                name = text.value();
                Ok(())
            } else if meta.path.is_ident(words::DEFAULT) {
                chosen = true;
                Ok(())
            } else {
                Err(meta.error("`option` takes `name` or `default`"))
            }
        })?;
    }
    Ok((name, chosen))
}

fn expand_variant(input: &DeriveInput) -> syn::Result<Tokens> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "only an enum derives `Variant`",
        ));
    };
    let derived = quote!(::balaur_core::components::derived);
    let toml = quote!(#derived::toml);
    let ident = &input.ident;
    let mut options = Vec::new();
    let mut variants = Vec::new();
    let mut records = Vec::new();
    let mut docs = Vec::new();
    let mut default = None;
    for variant in &data.variants {
        let record = match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => Some(&fields.unnamed[0].ty),
            Fields::Unit => None,
            _ => {
                return Err(syn::Error::new(
                    variant.span(),
                    "a variant holds one record, `Box(BoxShape)`, or nothing",
                ));
            }
        };
        let (name, chosen) = variant_option(variant)?;
        if chosen {
            default = Some(variants.len());
        }
        options.push(name);
        variants.push(&variant.ident);
        records.push(record);
        docs.push(doc_of(&variant.attrs));
    }
    if variants.is_empty() {
        return Err(syn::Error::new(
            input.ident.span(),
            "a variant enum needs one variant",
        ));
    }
    let chosen_at = default.unwrap_or(0);
    let chosen = variants[chosen_at];
    let chosen_value = if records[chosen_at].is_some() {
        quote!(Self::#chosen(::core::default::Default::default()))
    } else {
        quote!(Self::#chosen)
    };
    let VariantArms {
        patterns,
        describes,
        to_values,
        from_values,
    } = variant_arms(&derived, &variants, &records, &options, &docs);
    Ok(quote! {
        impl #ident {
            /// Every variant's name, in the order the enum declares them.
            pub const OPTIONS: &'static [&'static str] = &[#(#options),*];

            /// The variant's name, as a file and a script spell its `kind`.
            #[must_use]
            pub const fn option(&self) -> &'static str {
                match self {
                    #(#patterns => #options,)*
                }
            }
        }

        impl ::core::default::Default for #ident {
            fn default() -> Self {
                #chosen_value
            }
        }

        impl #derived::Property for #ident {
            fn describe(spec: &mut #toml::map::Map<::std::string::String, #toml::Value>) {
                let mut variants = #toml::map::Map::new();
                #(variants.insert(::std::string::String::from(#options), #describes);)*
                #derived::describe_variant(spec, variants);
            }

            fn to_value(&self) -> #toml::Value {
                match self {
                    #(#to_values,)*
                }
            }

            fn from_value(value: &#toml::Value) -> #derived::Result<Self> {
                match #derived::variant_kind(value, Self::OPTIONS)? {
                    #(#from_values,)*
                    _ => ::core::unreachable!("variant_kind answers one of OPTIONS"),
                }
            }
        }
    })
}

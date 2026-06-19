use varnish::vmod;

fn main() {}

#[vmod]
mod acl {
    use varnish::vcl::Acl;

    pub fn return_opt_opt_acl(_v: Option<Option<Acl>>) {}
    pub fn return_opt_acl() -> Option<Acl> {
        todo!()
    }

    pub fn return_res_opt_acl() -> Result<Option<Acl>, &'static str> {
        todo!()
    }
}
